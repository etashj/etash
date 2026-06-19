use itertools::MultiPeek;
use std::str::Chars;

use super::token::{
    InputState,
    InputState::{Closed, Open},
    Openable,
    Openable::Word as WordOpen,
    PartialState, Token,
    Token::Word,
    WordSegment,
    WordSegment::{Expandable, Literal},
    WordSubQuote,
};

use super::quote::{consume_double_quote_body, consume_single_quote_body};

/// Merges two adjacent same-type segments at `seam` index, if possible.
/// Only call this at cross-line resume boundaries, not within a single parse.
pub fn merge_seam(buf: &mut Vec<WordSegment>, seam: usize) {
    if seam == 0 || seam >= buf.len() {
        return;
    }
    let can_merge = matches!(
        (&buf[seam - 1], &buf[seam]),
        (Expandable(_), Expandable(_)) | (Literal(_), Literal(_))
    );
    if can_merge {
        let right = buf.remove(seam);
        match (&mut buf[seam - 1], right) {
            (Expandable(l), Expandable(r)) => l.push_str(&r),
            (Literal(l), Literal(r)) => l.push_str(&r),
            _ => unreachable!(),
        }
    }
}

// What `consume_word` did when it stopped.
enum WordResult {
    Done,                            // hit a terminator or true EOF cleanly
    Continue,                        // trailing backslash — needs another line
    InQuote(WordSubQuote, Openable), // hit an unterminated quote mid-word
}

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

/// Drives `consume_word` to completion, handling the three outcomes
/// uniformly: push the finished word, stash partial state and bail with
/// `Open(...)`, or recurse into a sub-quote and keep going once it closes.
/// Returns `Some(open_state)` if the caller should return early.
pub fn finish_word(
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
    }
}
