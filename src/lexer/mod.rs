pub mod quote;
pub mod token;
pub mod word;

pub use WordSegment::Literal;
use quote::{consume_double_quote_body, consume_single_quote_body};
pub use token::*;

use itertools::multipeek;
use token::InputState::{Closed, Open};
use word::{finish_word, merge_seam};

use crate::lexer::{Openable::CmdAnd, Openable::CmdOr};

pub fn tokenize(
    input: &str,
    existing_tokens: &mut Option<Vec<Token>>,
    partial: &mut PartialState,
) -> (Vec<Token>, InputState) {
    let mut tokens = existing_tokens.take().unwrap_or_default();
    let mut chars = multipeek(input.chars());

    match std::mem::replace(partial, PartialState::None) {
        PartialState::InWord(mut buf) => {
            let seam = buf.len();
            match finish_word(&mut chars, &mut buf, &mut tokens, partial) {
                Some(open) => {
                    // Another continuation: merge seam inside the new InWord partial.
                    if let PartialState::InWord(ref mut new_buf) = *partial {
                        merge_seam(new_buf, seam);
                    }
                    return (tokens, open);
                }
                None => {
                    if let Some(Token::Word(segs)) = tokens.last_mut() {
                        merge_seam(segs, seam);
                    }
                }
            }
        }
        PartialState::InWordQuote(mut buf, sub) => {
            // resume the inner quote first
            let resumed = match sub {
                WordSubQuote::Single(mut qbuf) => {
                    match consume_single_quote_body(&mut chars, &mut qbuf) {
                        Open(o) => {
                            *partial = PartialState::InWordQuote(buf, WordSubQuote::Single(qbuf));
                            return (tokens, Open(o));
                        }
                        Closed => {
                            buf.push(Literal(qbuf));
                            true
                        }
                    }
                }
                WordSubQuote::Double(mut qbuf) => {
                    let qseam = qbuf.len();
                    match consume_double_quote_body(&mut chars, &mut qbuf) {
                        Open(o) => {
                            merge_seam(&mut qbuf, qseam);
                            *partial = PartialState::InWordQuote(buf, WordSubQuote::Double(qbuf));
                            return (tokens, Open(o));
                        }
                        Closed => {
                            merge_seam(&mut qbuf, qseam);
                            buf.extend(qbuf);
                            true
                        }
                    }
                }
            };
            if resumed {
                match finish_word(&mut chars, &mut buf, &mut tokens, partial) {
                    Some(open) => return (tokens, open),
                    None => {}
                }
            }
        }
        PartialState::None => {}
    }

    while let Some(&c) = chars.peek() {
        chars.reset_peek();
        match c {
            ' ' | '\t' | '\n' => {
                chars.next();
            }
            '|' => {
                chars.next();
                if chars.peek() == Some(&'|') {
                    chars.next();
                    tokens.push(Token::Or);
                } else {
                    tokens.push(Token::Pipe);
                    chars.reset_peek();
                }
            }
            '&' => {
                chars.next();
                if chars.peek() == Some(&'&') {
                    chars.next();
                    tokens.push(Token::And);
                } else {
                    tokens.push(Token::Background);
                    chars.reset_peek();
                }
            }
            '>' => {
                chars.next();
                let fd = pop_digit_word(&mut tokens);
                if chars.peek() == Some(&'>') {
                    chars.next();
                    tokens.push(Token::RedirectAppend(fd));
                } else if chars.peek() == Some(&'&') {
                    chars.next();
                    tokens.push(Token::RedirectDupOut(fd));
                    chars.reset_peek();
                } else {
                    tokens.push(Token::RedirectOut(fd));
                    chars.reset_peek();
                }
            }
            '<' => {
                chars.next();
                tokens.push(Token::RedirectIn);
            }
            ';' => {
                chars.next();
                tokens.push(Token::Semicolon);
            }
            '(' => {
                chars.next();
                tokens.push(Token::LParen);
            }
            ')' => {
                chars.next();
                tokens.push(Token::RParen);
            }
            _ => {
                // bare word, possibly containing embedded quotes/escapes
                let mut buf = Vec::new();
                if let Some(open) = finish_word(&mut chars, &mut buf, &mut tokens, partial) {
                    return (tokens, open);
                }
            }
        }
    }

    match tokens.last() {
        Some(Token::And) => return (tokens, Open(CmdAnd)),
        Some(Token::Or) => return (tokens, Open(CmdOr)),
        _ => return (tokens, Closed),
    }
}

/// If the last token is a bare all-digit word, pop and return its value as an fd number.
fn pop_digit_word(tokens: &mut Vec<Token>) -> Option<u32> {
    if let Some(Token::Word(segs)) = tokens.last() {
        if segs.len() == 1 {
            if let token::WordSegment::Expandable(s) = &segs[0] {
                if let Ok(n) = s.parse::<u32>() {
                    tokens.pop();
                    return Some(n);
                }
            }
        }
    }
    None
}
