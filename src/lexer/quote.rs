use itertools::MultiPeek;
use std::str::Chars;

use super::token::{
    InputState,
    InputState::{Closed, Open},
    Openable::{Dquote, Quote},
    WordSegment,
    WordSegment::{Expandable, Literal},
};

pub fn consume_single_quote_body(chars: &mut MultiPeek<Chars>, buf: &mut String) -> InputState {
    while let Some(c) = chars.next() {
        if c == '\'' {
            return Closed;
        }
        buf.push(c); // single quotes: zero escaping, everything literal
    }
    Open(Quote)
}

pub fn consume_double_quote_body(
    chars: &mut MultiPeek<Chars>,
    buf: &mut Vec<WordSegment>,
) -> InputState {
    let mut str = String::new();

    macro_rules! flush {
        () => {
            if !str.is_empty() {
                buf.push(Expandable(std::mem::take(&mut str)));
            }
        };
    }

    while let Some(c) = chars.next() {
        match c {
            '"' => {
                flush!();
                return Closed;
            }
            '\\' => match chars.next() {
                Some('"') => {
                    flush!();
                    buf.push(Literal("\"".to_string()));
                }
                Some('\\') => {
                    flush!();
                    buf.push(Literal("\\".to_string()));
                }
                Some('$') => {
                    flush!();
                    buf.push(Literal("$".to_string()));
                }
                Some('`') => {
                    flush!();
                    buf.push(Literal("`".to_string()));
                }
                Some('\n') => {} // backslash-newline: discard both, no flush
                Some(nc) => {
                    flush!();
                    buf.push(Literal(format!("\\{}", nc)));
                }
                None => {
                    flush!();
                    buf.push(Literal("\\".to_string()));
                    return Open(Dquote);
                }
            },
            _ => str.push(c),
        }
    }

    flush!();
    Open(Dquote)
}
