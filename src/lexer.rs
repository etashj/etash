use itertools::{MultiPeek, multipeek};
use std::str::Chars;

use crate::lexer::{
    InputState::{Closed, Open},
    Openable::{Dquote, Quote, Word as WordOpen},
    Token::Word,
    WordSegment::{Expandable, Literal},
};

#[derive(Debug, Clone, PartialEq)]
pub enum Openable {
    Dquote,
    Quote,
    Word, // mid-word backslash-newline continuation
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputState {
    Closed,         // Quote/word was completed
    Open(Openable), // hit EOF mid-token — ask for another line
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

/// A quote opened from inside a bare (unquoted) word, e.g. abc"def".
#[derive(Debug, Clone, PartialEq)]
pub enum WordSubQuote {
    Single(String),
    Double(Vec<WordSegment>),
}

/// What `consume_word` did when it stopped.
enum WordResult {
    Done,                            // hit a terminator or true EOF cleanly
    Continue,                        // trailing backslash — needs another line
    InQuote(WordSubQuote, Openable), // hit an unterminated quote mid-word
}

/// Carries in-progress token content across calls to `tokenize`, since a
/// quote or escaped word can span multiple lines of input.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum PartialState {
    #[default]
    None,
    InWord(Vec<WordSegment>),
    InWordQuote(Vec<WordSegment>, WordSubQuote),
}

pub fn tokenize(
    input: &str,
    existing_tokens: &mut Option<Vec<Token>>,
    partial: &mut PartialState,
) -> (Vec<Token>, InputState) {
    let mut tokens = existing_tokens.take().unwrap_or_default();
    let mut chars = multipeek(input.chars());

    match std::mem::replace(partial, PartialState::None) {
        PartialState::InWord(mut buf) => {
            match finish_word(&mut chars, &mut buf, &mut tokens, partial) {
                Some(open) => return (tokens, open),
                None => {}
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
                    match consume_double_quote_body(&mut chars, &mut qbuf) {
                        Open(o) => {
                            *partial = PartialState::InWordQuote(buf, WordSubQuote::Double(qbuf));
                            return (tokens, Open(o));
                        }
                        Closed => {
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
            _ => {
                // bare word, possibly containing embedded quotes/escapes
                let mut buf = Vec::new();
                if let Some(open) = finish_word(&mut chars, &mut buf, &mut tokens, partial) {
                    return (tokens, open);
                }
            }
        }
    }

    (tokens, Closed)
}

/// Drives `consume_word` to completion, handling the three outcomes
/// uniformly: push the finished word, stash partial state and bail with
/// `Open(...)`, or recurse into a sub-quote and keep going once it closes.
/// Returns `Some(open_state)` if the caller should return early.
fn finish_word(
    chars: &mut MultiPeek<Chars>,
    buf: &mut Vec<WordSegment>,
    tokens: &mut Vec<Token>,
    partial: &mut PartialState,
) -> Option<InputState> {
    loop {
        match consume_word(chars, buf) {
            WordResult::Done => {
                tokens.push(Word(std::mem::take(buf)));
                return None;
            }
            WordResult::Continue => {
                *partial = PartialState::InWord(std::mem::take(buf));
                return Some(Open(WordOpen));
            }
            WordResult::InQuote(sub, _kind) => match sub {
                WordSubQuote::Single(mut qbuf) => {
                    match consume_single_quote_body(chars, &mut qbuf) {
                        Open(o) => {
                            *partial = PartialState::InWordQuote(
                                std::mem::take(buf),
                                WordSubQuote::Single(qbuf),
                            );
                            return Some(Open(o));
                        }
                        Closed => {
                            buf.push(Literal(qbuf));
                            // loop again: keep scanning the rest of the word
                        }
                    }
                }
                WordSubQuote::Double(mut qbuf) => {
                    match consume_double_quote_body(chars, &mut qbuf) {
                        Open(o) => {
                            *partial = PartialState::InWordQuote(
                                std::mem::take(buf),
                                WordSubQuote::Double(qbuf),
                            );
                            return Some(Open(o));
                        }
                        Closed => {
                            buf.extend(qbuf);
                            // loop again: keep scanning the rest of the word
                        }
                    }
                }
            },
        }
        let _ = kind_unused(&kind_placeholder()); // no-op to keep `kind` warning-free if unused below
    }
}

// helper stubs to avoid unused-variable warnings in the snippet; remove in
// your actual file if you don't need them — `kind` above isn't otherwise
// used because we only need it when returning Open(o) from the sub-quote,
// not after it closes successfully.
fn kind_placeholder() -> () {}
fn kind_unused(_: &()) {}

/// Consumes as much of a bare (unquoted) word as possible. Handles:
/// - terminators (whitespace, metacharacters) → Done
/// - `\` + newline or `\` + EOF → Continue (request another line)
/// - `\` + any other char → literal escape, including escaped space,
///   which joins what would otherwise be two words into one
/// - `'` / `"` mid-word → hands off to InQuote, caller splices the result
///   back into the same word instead of treating it as a separate token
fn consume_word(chars: &mut MultiPeek<Chars>, buf: &mut Vec<WordSegment>) -> WordResult {
    let mut str = String::new();

    macro_rules! flush {
        () => {
            if !str.is_empty() {
                buf.push(Expandable(std::mem::take(&mut str)));
            }
        };
    }

    loop {
        match chars.peek() {
            Some(&c) => {
                chars.reset_peek();
                match c {
                    ' ' | '\t' | '\n' | '|' | '&' | '>' | '<' | ';' | '(' | ')' => {
                        flush!();
                        return WordResult::Done;
                    }
                    '\'' => {
                        flush!();
                        chars.next(); // consume opening '
                        let mut qbuf = String::new();
                        match consume_single_quote_body(chars, &mut qbuf) {
                            Closed => {
                                buf.push(Literal(qbuf));
                                // keep scanning the rest of the word
                            }
                            Open(o) => return WordResult::InQuote(WordSubQuote::Single(qbuf), o),
                        }
                    }
                    '"' => {
                        flush!();
                        chars.next(); // consume opening "
                        let mut v = Vec::new();
                        match consume_double_quote_body(chars, &mut v) {
                            Closed => {
                                buf.extend(v);
                                // keep scanning the rest of the word
                            }
                            Open(o) => return WordResult::InQuote(WordSubQuote::Double(v), o),
                        }
                    }
                    '\\' => {
                        chars.next(); // consume the backslash
                        match chars.next() {
                            Some('\n') => {
                                if chars.peek().is_none() {
                                    chars.reset_peek();
                                    flush!();
                                    return WordResult::Continue;
                                }
                                chars.reset_peek();
                                // more input already present on this line
                                // after the escaped newline — just continue
                            }
                            Some(nc) => {
                                // backslash escapes the next char literally,
                                // including space — this is what joins
                                // "two words" into one when unquoted
                                flush!();
                                buf.push(Literal(nc.to_string()));
                            }
                            None => {
                                // backslash was the absolute last character
                                flush!();
                                return WordResult::Continue;
                            }
                        }
                    }
                    _ => {
                        str.push(c);
                        chars.next();
                    }
                }
            }
            None => {
                flush!();
                return WordResult::Done;
            }
        }
    }
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
