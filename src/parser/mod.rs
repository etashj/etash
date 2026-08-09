pub mod ast;

use crate::lexer::{Token, WordSegment};
use ast::{AST, ParserError, Redirect};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Semicolon,
    And,
    Or,
    Pipe,
}

impl Op {
    fn precedence(self) -> u8 {
        match self {
            Op::Semicolon => 1,
            Op::And | Op::Or => 2,
            Op::Pipe => 3,
        }
    }
}

enum OpSlot {
    Operator(Op),
    LParenMarker,
}

struct Parser {
    operand_stack: Vec<AST>,
    op_stack: Vec<OpSlot>,
    working_argv: Vec<Vec<WordSegment>>,
    working_redirs: Vec<Redirect>,
}

impl Parser {
    fn new() -> Self {
        Parser {
            operand_stack: Vec::new(),
            op_stack: Vec::new(),
            working_argv: Vec::new(),
            working_redirs: Vec::new(),
        }
    }

    fn apply_top(&mut self) -> Result<(), ParserError> {
        let op = match self.op_stack.pop() {
            Some(OpSlot::Operator(op)) => op,
            Some(OpSlot::LParenMarker) => return Err(ParserError::UnmatchedParen),
            None => return Err(ParserError::UnexpectedEnd),
        };
        let right = self
            .operand_stack
            .pop()
            .ok_or(ParserError::MissingOperand)?;
        let left = self
            .operand_stack
            .pop()
            .ok_or(ParserError::MissingOperand)?;

        let node = match op {
            Op::Semicolon => match left {
                AST::Sequence(mut cmds) => {
                    cmds.push(right);
                    AST::Sequence(cmds)
                }
                other => AST::Sequence(vec![other, right]),
            },
            Op::And => AST::And(Box::new(left), Box::new(right)),
            Op::Or => AST::Or(Box::new(left), Box::new(right)),
            Op::Pipe => match left {
                AST::Pipeline(mut cmds) => {
                    cmds.push(right);
                    AST::Pipeline(cmds)
                }
                other => AST::Pipeline(vec![other, right]),
            },
        };
        self.operand_stack.push(node);
        Ok(())
    }

    fn flush(&mut self) {
        if !self.working_argv.is_empty() || !self.working_redirs.is_empty() {
            self.operand_stack.push(AST::Command {
                argv: std::mem::take(&mut self.working_argv),
                redirs: std::mem::take(&mut self.working_redirs),
            });
        }
    }

    fn push_op(&mut self, op: Op) -> Result<(), ParserError> {
        loop {
            match self.op_stack.last() {
                Some(OpSlot::Operator(top)) if top.precedence() >= op.precedence() => {
                    self.apply_top()?;
                }
                _ => break,
            }
        }
        self.op_stack.push(OpSlot::Operator(op));
        Ok(())
    }

    fn parse(mut self, tokens: Vec<Token>) -> Result<AST, ParserError> {
        let mut i = 0;
        while i < tokens.len() {
            let tok = &tokens[i];
            i += 1;

            match tok {
                Token::Word(segs) => {
                    self.working_argv.push(segs.clone());
                }

                Token::RedirectIn => match tokens.get(i) {
                    Some(Token::Word(segs)) => {
                        self.working_redirs.push(Redirect::In(segs.clone()));
                        i += 1;
                    }
                    _ => return Err(ParserError::UnexpectedEnd),
                },

                Token::RedirectOut(fd) => {
                    let fd = *fd;
                    match tokens.get(i) {
                        Some(Token::Word(segs)) => {
                            self.working_redirs.push(Redirect::Out(segs.clone(), fd));
                            i += 1;
                        }
                        _ => return Err(ParserError::UnexpectedEnd),
                    }
                }

                Token::RedirectAppend(fd) => {
                    let fd = *fd;
                    match tokens.get(i) {
                        Some(Token::Word(segs)) => {
                            self.working_redirs.push(Redirect::Append(segs.clone(), fd));
                            i += 1;
                        }
                        _ => return Err(ParserError::UnexpectedEnd),
                    }
                }

                Token::RedirectDupOut(fd) => {
                    let target = fd.unwrap_or(1);
                    match tokens.get(i) {
                        Some(Token::Word(segs)) if is_digit_word(segs) => {
                            let source = digit_word_value(segs);
                            self.working_redirs.push(Redirect::Dup { target, source });
                            i += 1;
                        }
                        _ => return Err(ParserError::UnexpectedEnd),
                    }
                }

                Token::Semicolon => {
                    self.flush();
                    self.push_op(Op::Semicolon)?;
                }
                Token::Pipe => {
                    self.flush();
                    self.push_op(Op::Pipe)?;
                }
                Token::And => {
                    self.flush();
                    self.push_op(Op::And)?;
                }
                Token::Or => {
                    self.flush();
                    self.push_op(Op::Or)?;
                }

                Token::Background => {
                    self.flush();
                    // Drain Pipe/And/Or so Background wraps the full pipeline/and-or
                    // list, not just the last command. e.g. `a | b &` →
                    // Background(Pipeline([a,b])), not Pipeline([a, Background(b)])
                    loop {
                        match self.op_stack.last() {
                            Some(OpSlot::Operator(top))
                                if top.precedence() > Op::Semicolon.precedence() =>
                            {
                                self.apply_top()?;
                            }
                            _ => break,
                        }
                    }
                    let top = self
                        .operand_stack
                        .pop()
                        .ok_or(ParserError::MissingOperand)?;
                    self.operand_stack.push(AST::Background(Box::new(top)));
                    // Push a synthetic ; so `a & b` → Sequence([Background(a), b])
                    if i < tokens.len() {
                        self.push_op(Op::Semicolon)?;
                    }
                }

                Token::LParen => {
                    self.op_stack.push(OpSlot::LParenMarker);
                }
                Token::RParen => {
                    self.flush();
                    loop {
                        match self.op_stack.last() {
                            Some(OpSlot::LParenMarker) => {
                                self.op_stack.pop();
                                break;
                            }
                            Some(OpSlot::Operator(_)) => {
                                self.apply_top()?;
                            }
                            None => return Err(ParserError::UnmatchedParen),
                        }
                    }
                    let inner = self
                        .operand_stack
                        .pop()
                        .ok_or(ParserError::MissingOperand)?;
                    self.operand_stack.push(AST::Subshell(Box::new(inner)));
                }
            }
        }

        self.flush();

        while !self.op_stack.is_empty() {
            self.apply_top()?;
        }

        if self.operand_stack.len() != 1 {
            return Err(ParserError::MissingOperand);
        }

        Ok(self.operand_stack.pop().unwrap())
    }
}

pub fn parse(tokens: Vec<Token>) -> Result<AST, ParserError> {
    Parser::new().parse(tokens)
}

fn is_digit_word(segs: &[WordSegment]) -> bool {
    segs.len() == 1
        && matches!(&segs[0], WordSegment::Expandable(s) if s.chars().all(|c| c.is_ascii_digit()))
}

fn digit_word_value(segs: &[WordSegment]) -> u32 {
    match &segs[0] {
        WordSegment::Expandable(s) => s.parse().unwrap_or(0),
        _ => 0,
    }
}
