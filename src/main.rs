use std::io::{Write, stdin, stdout};

use etash::executor::{execute, reap_jobs};
use etash::lexer::{
    InputState::Open,
    Openable::{CmdAnd, CmdOr, Dquote, Quote, Word},
    PartialState, tokenize,
};
use etash::parser::parse;
use etash::shell::Shell;

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
    let mut shell = Shell::new();
    shell.load_rc();

    loop {
        reap_jobs(&mut shell);
        print!("etash: ");
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
