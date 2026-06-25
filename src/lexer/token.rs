#[derive(Debug, Clone, PartialEq)]
pub enum Openable {
    Dquote,
    Quote,
    Word, // mid-word backslash-newline continuation
    CmdAnd,
    CmdOr,
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

/// Carries in-progress token content across calls to `tokenize`, since a
/// quote or escaped word can span multiple lines of input.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum PartialState {
    #[default]
    None,
    InWord(Vec<WordSegment>),
    InWordQuote(Vec<WordSegment>, WordSubQuote),
}
