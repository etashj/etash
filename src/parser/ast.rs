use crate::lexer::WordSegment;

#[derive(Debug, Clone, PartialEq)]
pub enum AST {
    Command {
        /// Each element is one argument word, stored as raw segments to be
        /// expanded at execution time by `Shell::expand_word`.
        argv: Vec<Vec<WordSegment>>,
        redirs: Vec<Redirect>,
    },
    Pipeline(Vec<AST>),
    Sequence(Vec<AST>),
    And(Box<AST>, Box<AST>),
    Or(Box<AST>, Box<AST>),
    Background(Box<AST>),
    Subshell(Box<AST>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Redirect {
    In(Vec<WordSegment>),
    Out(Vec<WordSegment>, Option<u32>),
    Append(Vec<WordSegment>, Option<u32>),
    /// `[n]>&m` — duplicate fd `source` into fd `target` (dup2(source, target))
    Dup { target: u32, source: u32 },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ParserError {
    UnmatchedParen,
    MissingOperand,
    UnexpectedEnd,
}
