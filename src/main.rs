use std::io::{Write, stdin, stdout};
extern crate libc;

use etash::executor::{execute, reap_jobs};
use etash::lexer::{
    InputState::Open,
    Openable::{CmdAnd, CmdOr, Dquote, Quote, Word},
    PartialState, tokenize,
};
use etash::parser::parse;
use etash::shell::Shell;

fn format_cwd(shell: &etash::shell::Shell) -> String {
    let cwd = shell.cwd.to_string_lossy();
    if let Some(home) = shell.get("HOME") {
        if let Some(rest) = cwd.strip_prefix(home) {
            return format!("~{rest}");
        }
    }
    cwd.into_owned()
}

fn read_line() -> String {
    let mut s = String::new();
    stdin().read_line(&mut s).expect("Did not enter string");
    if s.ends_with("\r\n") {
        s.truncate(s.len() - 2);
        s.push('\n');
    }
    s
}

fn main() {
    // Shell ignores Ctrl+C and Ctrl+\ — children get them via the terminal pgrp
    unsafe {
        libc::signal(libc::SIGINT, libc::SIG_IGN);
        libc::signal(libc::SIGQUIT, libc::SIG_IGN);
        libc::signal(libc::SIGTSTP, libc::SIG_IGN);
    }

    let mut shell = Shell::new();
    shell.load_rc();

    loop {
        reap_jobs(&mut shell);
        print!("{} ? ", format_cwd(&shell));
        let _ = stdout().flush();
        let s = read_line();
        if s.is_empty() {
            break;
        }

        let mut existing = None;
        let mut partial = PartialState::None;
        let (mut tokens, mut status) = tokenize(&s, &mut existing, &mut partial);

        while let Open(reason) = &status {
            match reason {
                Quote => print!("quote> "),
                Dquote => print!("dquote> "),
                Word => print!("> "),
                CmdAnd => print!("cmdand> "),
                CmdOr => print!("cmdor> "),
            }
            let _ = stdout().flush();

            // the newline the user just pressed is meaningful inside the quote
            let next_line = read_line();
            existing = Some(tokens);
            (tokens, status) = tokenize(&next_line, &mut existing, &mut partial);
        }

        match parse(tokens) {
            Ok(ast) => {
                let status = execute(&ast, &mut shell);
                shell.status = status;
            }
            Err(e) => eprintln!("parse error: {:?}", e),
        }
    }
}
