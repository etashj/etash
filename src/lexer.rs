use itertools::{MultiPeek, multipeek};
use std::str::Chars;

use crate::lexer::{
    InputState::{Closed, Open},
    Openable::{Dquote, Quote},
    Token::Word,
    WordSegment::{Expandable, Literal},
};

#[derive(Debug, Clone, PartialEq)]
pub enum Openable {
    Dquote,
    Quote,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputState {
    Closed,         // Quote was completed
    Open(Openable), // hit EOF mid-quote — ask for another line
}

#[derive(Debug, Clone, PartialEq)]
pub enum WordSegment {
    Literal(String),
    Expandable(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Word(Vec<WordSegment>),   // Command name or arg
    Pipe,                     // |
    RedirectIn,               //
    RedirectOut(Option<u32>), // >
    RedirectAppend,           // >>
    And,                      // &&
    Or,                       // ||
    Semicolon,                // ;
    Background,               // &
    LParen,                   // (
    RParen,                   // )
}

/// Carries an in-progress quote's accumulated content across calls to
/// `tokenize`, since a quote can span multiple lines of input.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum PartialState {
    #[default]
    None,
    InQuote(String),
    InDquote(Vec<WordSegment>),
}

pub fn tokenize(
    input: &str,
    existing_tokens: &mut Option<Vec<Token>>,
    partial: &mut PartialState,
) -> (Vec<Token>, InputState) {
    let mut tokens = existing_tokens.take().unwrap_or_default();
    let mut chars = multipeek(input.chars());

    // If we're resuming mid-quote, finish that before normal dispatch.
    // No opening delimiter to consume here — we're already inside it.
    match std::mem::replace(partial, PartialState::None) {
        PartialState::InQuote(mut buf) => match consume_single_quote_body(&mut chars, &mut buf) {
            Open(o) => {
                *partial = PartialState::InQuote(buf);
                return (tokens, Open(o));
            }
            Closed => tokens.push(Word(vec![Literal(buf)])),
        },
        PartialState::InDquote(mut v) => match consume_double_quote_body(&mut chars, &mut v) {
            Open(o) => {
                *partial = PartialState::InDquote(v);
                return (tokens, Open(o));
            }
            Closed => tokens.push(Word(v)),
        },
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
                chars.reset_peek();
                if chars.peek() == Some(&'|') {
                    chars.next();
                    tokens.push(Token::Or);
                } else {
                    tokens.push(Token::Pipe);
                }
            }
            '&' => {
                chars.next();
                chars.reset_peek();
                if chars.peek() == Some(&'&') {
                    chars.next();
                    tokens.push(Token::And);
                } else {
                    tokens.push(Token::Background);
                }
            }
            '>' => {
                chars.next();
                chars.reset_peek();
                if chars.peek() == Some(&'>') {
                    chars.next();
                    tokens.push(Token::RedirectAppend);
                } else {
                    tokens.push(Token::RedirectOut(None));
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
            '\'' => {
                let mut buf = String::new();
                match consume_single_quote_open(&mut chars, &mut buf) {
                    Open(o) => {
                        *partial = PartialState::InQuote(buf);
                        return (tokens, Open(o));
                    }
                    Closed => {
                        tokens.push(Word(vec![Literal(buf)]));
                    }
                }
            }
            '"' => {
                let mut v = Vec::new();
                match consume_double_quote_open(&mut chars, &mut v) {
                    Open(o) => {
                        *partial = PartialState::InDquote(v);
                        return (tokens, Open(o));
                    }
                    Closed => {
                        tokens.push(Word(v));
                    }
                }
            }
            _ => {
                let word = consume_word(&mut chars);
                tokens.push(Word(word));
            }
        }
    }

    (tokens, Closed)
}

/// Consumes a bare (unquoted) word: runs until whitespace or a shell
/// metacharacter. The whole thing is treated as a single Expandable segment.
/// (FD-prefix detection for redirects, e.g. `2>`, is not handled here yet.)
fn consume_word(chars: &mut MultiPeek<Chars>) -> Vec<WordSegment> {
    let mut str = String::new();
    loop {
        match chars.peek() {
            Some(&c) => {
                chars.reset_peek();
                match c {
                    ' ' | '\t' | '\n' | '|' | '&' | '>' | '<' | ';' | '(' | ')' | '\'' | '"' => {
                        break;
                    }
                    _ => {
                        str.push(c);
                        chars.next();
                    }
                }
            }
            None => {
                chars.reset_peek();
                break;
            }
        }
    }
    vec![Expandable(str)]
}

fn consume_single_quote_open(chars: &mut MultiPeek<Chars>, buf: &mut String) -> InputState {
    chars.next(); // consume opening '
    consume_single_quote_body(chars, buf)
}

fn consume_single_quote_body(chars: &mut MultiPeek<Chars>, buf: &mut String) -> InputState {
    while let Some(c) = chars.next() {
        if c == '\'' {
            return Closed;
        }
        buf.push(c); // single quotes: zero escaping, everything literal
    }
    Open(Quote)
}

fn consume_double_quote_open(
    chars: &mut MultiPeek<Chars>,
    buf: &mut Vec<WordSegment>,
) -> InputState {
    chars.next(); // consume opening "
    consume_double_quote_body(chars, buf)
}

fn consume_double_quote_body(
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
                    // backslash has no special meaning before other chars:
                    // keep both characters together as one literal run
                    flush!();
                    buf.push(Literal(format!("\\{}", nc)));
                }
                None => {
                    // line ended right after the backslash — no real
                    // newline was consumed, so this is just a literal
                    // backslash inside a still-unterminated quote
                    flush!();
                    buf.push(Literal("\\".to_string()));
                    return Open(Dquote);
                }
            },
            _ => str.push(c), // plain text, still eligible for expansion
        }
    }

    flush!();
    Open(Dquote) // unterminated
}
