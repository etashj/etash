mod builtin;
mod redir;

extern crate libc;

use crate::lexer::WordSegment;
use crate::parser::ast::{AST, Redirect};
use crate::shell::Shell;

use std::os::unix::process::CommandExt;
use std::process::Command;

/// Polls all background jobs and reaps any that have finished.
pub fn reap_jobs(shell: &mut Shell) {
    let done: Vec<u32> = shell
        .iter_jobs_mut()
        .filter_map(|(pid, job, child)| match child.try_wait() {
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
        AST::Pipeline(_v) => 1,
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
        AST::Subshell(_b) => 1,
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
    // Store child so we can move it into the job table if it stops.
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

fn run_cmd(
    shell: &mut Shell,
    argv: &Vec<Vec<WordSegment>>,
    redirs: &Vec<Redirect>,
    bg: bool,
) -> i32 {
    let expanded_argv: Vec<String> = argv
        .iter()
        .map(|word_vec| shell.expand_word(word_vec.as_slice()))
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
