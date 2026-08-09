mod builtin;
mod redir;

use crate::lexer::WordSegment;
use crate::parser::ast::{AST, Redirect};
use crate::shell::Shell;

extern crate libc;

use std::os::unix::process::CommandExt;
use std::process::Command;

/// Polls all background jobs and reaps any that have finished.
pub fn reap_jobs(shell: &mut Shell) {
    let done: Vec<u32> = shell
        .iter_jobs_mut()
        .filter_map(|(pid, child)| match child.try_wait() {
            Ok(Some(_)) => Some(*pid),
            _ => None,
        })
        .collect();

    for pid in done {
        if let Some(mut child) = shell.remove_ps(pid) {
            let code = child.wait().map(|s| s.code().unwrap_or(1)).unwrap_or(1);
            println!("[done] pid={pid} exit={code}");
        }
    }
}

/// Executes an AST node and returns the exit status.
pub fn execute(ast: &AST, shell: &mut Shell) -> i32 {
    match ast {
        AST::Pipeline(v) => {
            return 1;
        }
        AST::Sequence(v) => {
            let mut last = 0;

            for a in v {
                last = execute(a, shell);
            }

            return last;
        }
        AST::And(b1, b2) => {
            let res = execute(b1, shell);
            if res == 0 {
                return execute(b2, shell);
            } else {
                return res;
            }
        }
        AST::Or(b1, b2) => {
            let res = execute(b1, shell);
            if res != 0 {
                return execute(b2, shell);
            } else {
                return res;
            }
        }
        AST::Subshell(b) => {
            return 1;
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
                println!("[running] pid={} {}", c.id(), expanded_argv[0]);
                shell.add_ps(c);
                0
            }
            Err(e) => {
                eprintln!("{}: {}", expanded_argv[0], e);
                1
            }
        }
    } else {
        let code = cmd.status().map(|s| s.code().unwrap_or(1)).unwrap_or(127);
        shell.status = code;
        code
    }
}
