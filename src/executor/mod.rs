mod builtin;
mod redir;

extern crate libc;

use crate::lexer::WordSegment;
use crate::parser::ast::{AST, Redirect};
use crate::shell::Shell;

use std::os::unix::io::FromRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

fn glob_expand(word: String) -> Vec<String> {
    if !word.contains(['*', '?', '[']) {
        return vec![word];
    }
    let matches: Vec<String> = glob::glob(&word)
        .into_iter()
        .flatten()
        .filter_map(|p| p.ok())
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    if matches.is_empty() { vec![word] } else { matches }
}

/// Polls all background jobs and reaps any that have finished.
pub fn reap_jobs(shell: &mut Shell) {
    let done: Vec<u32> = shell
        .iter_jobs_mut()
        .filter_map(|(pid, _job, child)| match child.try_wait() {
            Ok(Some(_)) => Some(pid),
            _ => None,
        })
        .collect();

    for pid in done {
        if let Some((job, mut child)) = shell.remove_pid(pid) {
            let code = child.wait().map(|s| s.code().unwrap_or(1)).unwrap_or(1);
            println!("[{job}] done  pid={pid} exit={code}");
        }
    }
}

/// Executes an AST node and returns the exit status.
pub fn execute(ast: &AST, shell: &mut Shell) -> i32 {
    match ast {
        AST::Pipeline(cmds) => run_pipeline(shell, cmds),
        AST::Sequence(v) => {
            let mut last = 0;
            for a in v {
                last = execute(a, shell);
            }
            last
        }
        AST::And(b1, b2) => {
            let res = execute(b1, shell);
            if res == 0 { execute(b2, shell) } else { res }
        }
        AST::Or(b1, b2) => {
            let res = execute(b1, shell);
            if res != 0 { execute(b2, shell) } else { res }
        }
        AST::Subshell(b) => {
            let pid = unsafe { libc::fork() };
            match pid {
                -1 => 1,
                0 => {
                    let code = execute(b, shell);
                    std::process::exit(code);
                }
                child_pid => {
                    let mut wstatus: libc::c_int = 0;
                    unsafe { libc::waitpid(child_pid, &mut wstatus, libc::WUNTRACED) };
                    if libc::WIFEXITED(wstatus) { libc::WEXITSTATUS(wstatus) } else { 1 }
                }
            }
        }
        AST::Background(b) => {
            if let AST::Command { argv, redirs } = b.as_ref() {
                return run_cmd(shell, argv, redirs, true);
            }
            1
        }
        AST::Command { argv, redirs } => run_cmd(shell, argv, redirs, false),
    }
}

/// Waits on a spawned child, handling stop (Ctrl+Z) by adding it to the job
/// table. Returns the exit/signal status.
pub fn wait_foreground(shell: &mut Shell, child: std::process::Child) -> i32 {
    let pid = child.id() as libc::pid_t;
    let mut child = Some(child);

    loop {
        let mut wstatus: libc::c_int = 0;
        let ret = unsafe { libc::waitpid(pid, &mut wstatus, libc::WUNTRACED) };
        if ret == -1 {
            return 1;
        }
        if libc::WIFEXITED(wstatus) {
            return libc::WEXITSTATUS(wstatus);
        }
        if libc::WIFSTOPPED(wstatus) {
            let job = shell.add_job(child.take().unwrap());
            eprintln!("[{job}] stopped  pid={pid}");
            return 128 + libc::WSTOPSIG(wstatus);
        }
        if libc::WIFSIGNALED(wstatus) {
            return 128 + libc::WTERMSIG(wstatus);
        }
    }
}

fn run_pipeline(shell: &mut Shell, cmds: &[AST]) -> i32 {
    let n = cmds.len();
    if n == 0 { return 0; }
    if n == 1 { return execute(&cmds[0], shell); }

    // Create n-1 pipes: pipes[i] connects stage i stdout → stage i+1 stdin
    let mut pipes: Vec<[libc::c_int; 2]> = Vec::with_capacity(n - 1);
    for _ in 0..n - 1 {
        let mut fds = [0i32; 2];
        if unsafe { libc::pipe(fds.as_mut_ptr()) } == -1 {
            eprintln!("pipe: {}", std::io::Error::last_os_error());
            return 1;
        }
        pipes.push(fds); // [read_fd, write_fd]
    }

    let mut children: Vec<std::process::Child> = Vec::with_capacity(n);

    for (i, stage) in cmds.iter().enumerate() {
        let stdin_fd = if i == 0 { None } else { Some(pipes[i - 1][0]) };
        let stdout_fd = if i < n - 1 { Some(pipes[i][1]) } else { None };

        let AST::Command { argv, redirs } = stage else {
            eprintln!("etash: complex pipeline stages not yet supported");
            continue;
        };

        let expanded_argv: Vec<String> = argv
            .iter()
            .map(|w| shell.expand_word(w))
            .flat_map(glob_expand)
            .collect();

        if expanded_argv.is_empty() { continue; }

        let mut cmd = Command::new(&expanded_argv[0]);
        cmd.args(&expanded_argv[1..]);

        // Wire up pipe ends — Stdio::from_raw_fd takes ownership, so the fd
        // is closed in the parent when `cmd` drops at the end of this iteration.
        if let Some(fd) = stdin_fd {
            cmd.stdin(unsafe { Stdio::from_raw_fd(fd) });
        }
        if let Some(fd) = stdout_fd {
            cmd.stdout(unsafe { Stdio::from_raw_fd(fd) });
        }

        unsafe {
            cmd.pre_exec(|| {
                libc::signal(libc::SIGINT, libc::SIG_DFL);
                libc::signal(libc::SIGQUIT, libc::SIG_DFL);
                libc::signal(libc::SIGTSTP, libc::SIG_DFL);
                Ok(())
            });
        }

        if let Err(e) = redir::apply_redirects(&mut cmd, redirs, shell) {
            eprintln!("redirect error: {e}");
            continue;
        }

        match cmd.spawn() {
            Ok(child) => children.push(child),
            Err(e) => eprintln!("{}: {e}", expanded_argv[0]),
        }
        // `cmd` drops here → closes its pipe fds in the parent
    }

    // Wait for all stages; return exit code of the last one
    let mut last = 0;
    for mut child in children {
        if let Ok(status) = child.wait() {
            last = status.code().unwrap_or(1);
        }
    }
    last
}

fn run_cmd(
    shell: &mut Shell,
    argv: &Vec<Vec<WordSegment>>,
    redirs: &Vec<Redirect>,
    bg: bool,
) -> i32 {
    let expanded_argv: Vec<String> = argv
        .iter()
        .map(|word_vec| shell.expand_word(word_vec.as_slice()))
        .flat_map(glob_expand)
        .collect();

    if expanded_argv.is_empty() {
        return 1;
    }

    if let Some(code) = builtin::run_builtin(&expanded_argv, shell) {
        return code;
    }

    let mut cmd = Command::new(&expanded_argv[0]);
    cmd.args(&expanded_argv[1..]);
    unsafe {
        cmd.pre_exec(|| {
            libc::signal(libc::SIGINT, libc::SIG_DFL);
            libc::signal(libc::SIGQUIT, libc::SIG_DFL);
            libc::signal(libc::SIGTSTP, libc::SIG_DFL);
            Ok(())
        });
    }

    if let Err(e) = redir::apply_redirects(&mut cmd, redirs, shell) {
        eprintln!("redirect error: {e}");
        return 1;
    }

    if bg {
        match cmd.spawn() {
            Ok(c) => {
                let pid = c.id();
                let job = shell.add_job(c);
                println!("[{job}] running  pid={pid} {}", expanded_argv[0]);
                0
            }
            Err(e) => {
                eprintln!("{}: {}", expanded_argv[0], e);
                1
            }
        }
    } else {
        match cmd.spawn() {
            Ok(child) => {
                let code = wait_foreground(shell, child);
                shell.status = code;
                code
            }
            Err(e) => {
                eprintln!("{}: {}", expanded_argv[0], e);
                127
            }
        }
    }
}
