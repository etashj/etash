use std::io::{Write, stdin, stdout};
mod lexer;
use crate::lexer::{
    InputState::Open,
    Openable::{Dquote, Quote},
    PartialState,
};
use lexer::tokenize;

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
    loop {
        print!("etash: ");
        let _ = stdout().flush();
        let s = read_line();

        let mut existing = None;
        let mut partial = PartialState::None;
        let (mut tokens, mut status) = tokenize(&s, &mut existing, &mut partial);

        while let Open(reason) = &status {
            match reason {
                Quote => print!("quote> "),
                Dquote => print!("dquote> "),
            }
            let _ = stdout().flush();

            // the newline the user just pressed is meaningful inside the quote
            let next_line = read_line();
            existing = Some(tokens);
            (tokens, status) = tokenize(&next_line, &mut existing, &mut partial);
        }

        println!("{:?} - {:?}", status, tokens);
    }
}
