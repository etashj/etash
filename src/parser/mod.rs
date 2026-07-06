pub mod ast;

use crate::lexer::{Token, WordSegment};
use ast::{AST, ParserError, Redirect};

/// Operators that combine two finished commands/pipelines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Semicolon,
    And,
    Or,
    Pipe,
}

impl Op {
    /// Higher = binds tighter.
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

pub fn parse(tokens: Vec<Token>) -> Result<AST, ParserError> {
    let mut operand_stack: Vec<AST> = Vec::new();
    let mut op_stack: Vec<OpSlot> = Vec::new();
    let mut working_argv: Vec<Vec<WordSegment>> = Vec::new();
    let mut working_redirs: Vec<Redirect> = Vec::new();

    fn apply_top(
        operand_stack: &mut Vec<AST>,
        op_stack: &mut Vec<OpSlot>,
    ) -> Result<(), ParserError> {
        let op = match op_stack.pop() {
            Some(OpSlot::Operator(op)) => op,
            Some(OpSlot::LParenMarker) => return Err(ParserError::UnmatchedParen),
            None => return Err(ParserError::UnexpectedEnd),
        };
        let right = operand_stack.pop().ok_or(ParserError::MissingOperand)?;
        let left = operand_stack.pop().ok_or(ParserError::MissingOperand)?;

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
        operand_stack.push(node);
        Ok(())
    }

    fn flush(
        working_argv: &mut Vec<Vec<WordSegment>>,
        working_redirs: &mut Vec<Redirect>,
        operand_stack: &mut Vec<AST>,
    ) {
        if !working_argv.is_empty() || !working_redirs.is_empty() {
            operand_stack.push(AST::Command {
                argv: std::mem::take(working_argv),
                redirs: std::mem::take(working_redirs),
            });
        }
    }

    fn push_op(
        op: Op,
        operand_stack: &mut Vec<AST>,
        op_stack: &mut Vec<OpSlot>,
    ) -> Result<(), ParserError> {
        loop {
            match op_stack.last() {
                Some(OpSlot::Operator(top)) if top.precedence() >= op.precedence() => {
                    apply_top(operand_stack, op_stack)?;
                }
                _ => break,
            }
        }
        op_stack.push(OpSlot::Operator(op));
        Ok(())
    }

    let mut i = 0;
    while i < tokens.len() {
        let tok = &tokens[i];
        i += 1;

        match tok {
            Token::Word(segs) => {
                working_argv.push(segs.clone());
            }

            Token::RedirectIn => match tokens.get(i) {
                Some(Token::Word(segs)) => {
                    working_redirs.push(Redirect::In(segs.clone()));
                    i += 1;
                }
                _ => return Err(ParserError::UnexpectedEnd),
            },

            Token::RedirectOut(fd) => {
                let fd = *fd;
                match tokens.get(i) {
                    Some(Token::Word(segs)) => {
                        working_redirs.push(Redirect::Out(segs.clone(), fd));
                        i += 1;
                    }
                    _ => return Err(ParserError::UnexpectedEnd),
                }
            }

            Token::RedirectAppend => match tokens.get(i) {
                Some(Token::Word(segs)) => {
                    working_redirs.push(Redirect::Append(segs.clone(), None));
                    i += 1;
                }
                _ => return Err(ParserError::UnexpectedEnd),
            },

            Token::Semicolon => {
                flush(&mut working_argv, &mut working_redirs, &mut operand_stack);
                push_op(Op::Semicolon, &mut operand_stack, &mut op_stack)?;
            }
            Token::Pipe => {
                flush(&mut working_argv, &mut working_redirs, &mut operand_stack);
                push_op(Op::Pipe, &mut operand_stack, &mut op_stack)?;
            }
            Token::And => {
                flush(&mut working_argv, &mut working_redirs, &mut operand_stack);
                push_op(Op::And, &mut operand_stack, &mut op_stack)?;
            }
            Token::Or => {
                flush(&mut working_argv, &mut working_redirs, &mut operand_stack);
                push_op(Op::Or, &mut operand_stack, &mut op_stack)?;
            }

            Token::Background => {
                flush(&mut working_argv, &mut working_redirs, &mut operand_stack);
                // Drain Pipe/And/Or (precedence > Semicolon) so that Background
                // wraps the full and-or list/pipeline, not just the last command.
                // e.g. `a | b &` → Background(Pipeline([a,b])), not Pipeline([a, Background(b)])
                loop {
                    match op_stack.last() {
                        Some(OpSlot::Operator(top))
                            if top.precedence() > Op::Semicolon.precedence() =>
                        {
                            apply_top(&mut operand_stack, &mut op_stack)?;
                        }
                        _ => break,
                    }
                }
                let top = operand_stack.pop().ok_or(ParserError::MissingOperand)?;
                operand_stack.push(AST::Background(Box::new(top)));
                // Treat & as a sequence separator when more tokens follow,
                // so `a & b` parses as Sequence([Background(a), b]).
                if i < tokens.len() {
                    push_op(Op::Semicolon, &mut operand_stack, &mut op_stack)?;
                }
            }

            Token::LParen => {
                op_stack.push(OpSlot::LParenMarker);
            }
            Token::RParen => {
                flush(&mut working_argv, &mut working_redirs, &mut operand_stack);
                loop {
                    match op_stack.last() {
                        Some(OpSlot::LParenMarker) => {
                            op_stack.pop();
                            break;
                        }
                        Some(OpSlot::Operator(_)) => {
                            apply_top(&mut operand_stack, &mut op_stack)?;
                        }
                        None => return Err(ParserError::UnmatchedParen),
                    }
                }
                let inner = operand_stack.pop().ok_or(ParserError::MissingOperand)?;
                operand_stack.push(AST::Subshell(Box::new(inner)));
            }
        }
    }

    flush(&mut working_argv, &mut working_redirs, &mut operand_stack);

    while !op_stack.is_empty() {
        apply_top(&mut operand_stack, &mut op_stack)?;
    }

    if operand_stack.len() != 1 {
        return Err(ParserError::MissingOperand);
    }

    Ok(operand_stack.pop().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::{Token, WordSegment};
    use ast::{AST, ParserError, Redirect};

    // Build a one-segment Literal word token (convenient for tests;
    // real lexer uses Expandable for unquoted text).
    fn w(s: &str) -> Token {
        Token::Word(vec![WordSegment::Literal(s.to_string())])
    }

    fn segs(s: &str) -> Vec<WordSegment> {
        vec![WordSegment::Literal(s.to_string())]
    }

    fn cmd(argv: &[&str]) -> AST {
        AST::Command {
            argv: argv.iter().map(|s| segs(s)).collect(),
            redirs: vec![],
        }
    }

    fn cmd_r(argv: &[&str], redirs: Vec<Redirect>) -> AST {
        AST::Command {
            argv: argv.iter().map(|s| segs(s)).collect(),
            redirs,
        }
    }

    // --- basic command ---

    #[test]
    fn single_word() {
        assert_eq!(parse(vec![w("echo")]), Ok(cmd(&["echo"])));
    }

    #[test]
    fn multi_word_command() {
        assert_eq!(
            parse(vec![w("echo"), w("hello"), w("world")]),
            Ok(cmd(&["echo", "hello", "world"]))
        );
    }

    // --- pipeline ---

    #[test]
    fn two_stage_pipeline() {
        let tokens = vec![w("echo"), w("hello"), Token::Pipe, w("grep"), w("h")];
        assert_eq!(
            parse(tokens),
            Ok(AST::Pipeline(vec![
                cmd(&["echo", "hello"]),
                cmd(&["grep", "h"])
            ]))
        );
    }

    #[test]
    fn three_stage_pipeline_is_flat() {
        let tokens = vec![w("a"), Token::Pipe, w("b"), Token::Pipe, w("c")];
        assert_eq!(
            parse(tokens),
            Ok(AST::Pipeline(vec![cmd(&["a"]), cmd(&["b"]), cmd(&["c"])]))
        );
    }

    // --- sequence ---

    #[test]
    fn two_command_sequence() {
        let tokens = vec![w("echo"), w("a"), Token::Semicolon, w("echo"), w("b")];
        assert_eq!(
            parse(tokens),
            Ok(AST::Sequence(vec![cmd(&["echo", "a"]), cmd(&["echo", "b"])]))
        );
    }

    #[test]
    fn three_command_sequence_is_flat() {
        let tokens = vec![w("a"), Token::Semicolon, w("b"), Token::Semicolon, w("c")];
        assert_eq!(
            parse(tokens),
            Ok(AST::Sequence(vec![cmd(&["a"]), cmd(&["b"]), cmd(&["c"])]))
        );
    }

    // --- boolean operators ---

    #[test]
    fn and_operator() {
        let tokens = vec![w("cmd1"), Token::And, w("cmd2")];
        assert_eq!(
            parse(tokens),
            Ok(AST::And(
                Box::new(cmd(&["cmd1"])),
                Box::new(cmd(&["cmd2"]))
            ))
        );
    }

    #[test]
    fn or_operator() {
        let tokens = vec![w("cmd1"), Token::Or, w("cmd2")];
        assert_eq!(
            parse(tokens),
            Ok(AST::Or(
                Box::new(cmd(&["cmd1"])),
                Box::new(cmd(&["cmd2"]))
            ))
        );
    }

    #[test]
    fn pipe_precedence_over_and() {
        // a | b && c  →  And(Pipeline([a,b]), c)
        let tokens = vec![w("a"), Token::Pipe, w("b"), Token::And, w("c")];
        assert_eq!(
            parse(tokens),
            Ok(AST::And(
                Box::new(AST::Pipeline(vec![cmd(&["a"]), cmd(&["b"])])),
                Box::new(cmd(&["c"]))
            ))
        );
    }

    // --- background ---

    #[test]
    fn background_single_command() {
        let tokens = vec![w("sleep"), w("10"), Token::Background];
        assert_eq!(
            parse(tokens),
            Ok(AST::Background(Box::new(cmd(&["sleep", "10"]))))
        );
    }

    #[test]
    fn background_wraps_full_pipeline() {
        // a | b &  →  Background(Pipeline([a,b])), not Pipeline([a, Background(b)])
        let tokens = vec![w("a"), Token::Pipe, w("b"), Token::Background];
        assert_eq!(
            parse(tokens),
            Ok(AST::Background(Box::new(AST::Pipeline(vec![
                cmd(&["a"]),
                cmd(&["b"])
            ]))))
        );
    }

    #[test]
    fn background_wraps_full_and_list() {
        // a && b &  →  Background(And(a, b))
        let tokens = vec![w("a"), Token::And, w("b"), Token::Background];
        assert_eq!(
            parse(tokens),
            Ok(AST::Background(Box::new(AST::And(
                Box::new(cmd(&["a"])),
                Box::new(cmd(&["b"]))
            ))))
        );
    }

    #[test]
    fn background_followed_by_command_makes_sequence() {
        // a & b  →  Sequence([Background(a), b])
        let tokens = vec![w("a"), Token::Background, w("b")];
        assert_eq!(
            parse(tokens),
            Ok(AST::Sequence(vec![
                AST::Background(Box::new(cmd(&["a"]))),
                cmd(&["b"])
            ]))
        );
    }

    #[test]
    fn multiple_backgrounds_are_flat_sequence() {
        // a & b & c  →  Sequence([Background(a), Background(b), c])
        let tokens = vec![
            w("a"),
            Token::Background,
            w("b"),
            Token::Background,
            w("c"),
        ];
        assert_eq!(
            parse(tokens),
            Ok(AST::Sequence(vec![
                AST::Background(Box::new(cmd(&["a"]))),
                AST::Background(Box::new(cmd(&["b"]))),
                cmd(&["c"])
            ]))
        );
    }

    // --- subshell ---

    #[test]
    fn subshell() {
        let tokens = vec![Token::LParen, w("echo"), w("hi"), Token::RParen];
        assert_eq!(
            parse(tokens),
            Ok(AST::Subshell(Box::new(cmd(&["echo", "hi"]))))
        );
    }

    #[test]
    fn subshell_with_pipeline() {
        let tokens = vec![
            Token::LParen,
            w("a"),
            Token::Pipe,
            w("b"),
            Token::RParen,
        ];
        assert_eq!(
            parse(tokens),
            Ok(AST::Subshell(Box::new(AST::Pipeline(vec![
                cmd(&["a"]),
                cmd(&["b"])
            ]))))
        );
    }

    // --- redirects ---

    #[test]
    fn redirect_in() {
        let tokens = vec![w("cat"), Token::RedirectIn, w("file.txt")];
        assert_eq!(
            parse(tokens),
            Ok(cmd_r(&["cat"], vec![Redirect::In(segs("file.txt"))]))
        );
    }

    #[test]
    fn redirect_out() {
        let tokens = vec![w("echo"), w("hi"), Token::RedirectOut(None), w("out.txt")];
        assert_eq!(
            parse(tokens),
            Ok(cmd_r(
                &["echo", "hi"],
                vec![Redirect::Out(segs("out.txt"), None)]
            ))
        );
    }

    #[test]
    fn redirect_out_with_fd() {
        let tokens = vec![w("cmd"), Token::RedirectOut(Some(2)), w("err.txt")];
        assert_eq!(
            parse(tokens),
            Ok(cmd_r(&["cmd"], vec![Redirect::Out(segs("err.txt"), Some(2))]))
        );
    }

    #[test]
    fn redirect_append() {
        let tokens = vec![w("echo"), w("hi"), Token::RedirectAppend, w("log.txt")];
        assert_eq!(
            parse(tokens),
            Ok(cmd_r(
                &["echo", "hi"],
                vec![Redirect::Append(segs("log.txt"), None)]
            ))
        );
    }

    #[test]
    fn redirect_in_and_out() {
        let tokens = vec![
            w("sort"),
            Token::RedirectIn,
            w("in.txt"),
            Token::RedirectOut(None),
            w("out.txt"),
        ];
        assert_eq!(
            parse(tokens),
            Ok(cmd_r(
                &["sort"],
                vec![
                    Redirect::In(segs("in.txt")),
                    Redirect::Out(segs("out.txt"), None),
                ]
            ))
        );
    }

    // --- error cases ---

    #[test]
    fn unmatched_rparen() {
        assert_eq!(parse(vec![Token::RParen]), Err(ParserError::UnmatchedParen));
    }

    #[test]
    fn unmatched_lparen_extra_rparen() {
        assert_eq!(
            parse(vec![
                Token::LParen,
                w("echo"),
                Token::RParen,
                Token::RParen
            ]),
            Err(ParserError::UnmatchedParen)
        );
    }

    #[test]
    fn redirect_missing_target() {
        assert_eq!(
            parse(vec![w("cat"), Token::RedirectIn]),
            Err(ParserError::UnexpectedEnd)
        );
    }

    #[test]
    fn empty_tokens_is_error() {
        assert_eq!(parse(vec![]), Err(ParserError::MissingOperand));
    }
}
