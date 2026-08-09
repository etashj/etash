use etash::lexer::{Token, WordSegment};
use etash::parser::ast::{AST, ParserError, Redirect};
use etash::parser::parse;

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

mod basic_command {
    use super::*;

    #[test]
    fn single_word() {
        assert_eq!(parse(vec![w("echo")]), Ok(cmd(&["echo"])));
    }

    #[test]
    fn multi_word() {
        assert_eq!(
            parse(vec![w("echo"), w("hello"), w("world")]),
            Ok(cmd(&["echo", "hello", "world"]))
        );
    }

    #[test]
    fn empty_is_error() {
        assert_eq!(parse(vec![]), Err(ParserError::MissingOperand));
    }
}

mod pipeline {
    use super::*;

    #[test]
    fn two_stage() {
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
    fn three_stage_is_flat() {
        let tokens = vec![w("a"), Token::Pipe, w("b"), Token::Pipe, w("c")];
        assert_eq!(
            parse(tokens),
            Ok(AST::Pipeline(vec![cmd(&["a"]), cmd(&["b"]), cmd(&["c"])]))
        );
    }
}

mod sequence {
    use super::*;

    #[test]
    fn two_commands() {
        let tokens = vec![w("echo"), w("a"), Token::Semicolon, w("echo"), w("b")];
        assert_eq!(
            parse(tokens),
            Ok(AST::Sequence(vec![cmd(&["echo", "a"]), cmd(&["echo", "b"])]))
        );
    }

    #[test]
    fn three_commands_is_flat() {
        let tokens = vec![w("a"), Token::Semicolon, w("b"), Token::Semicolon, w("c")];
        assert_eq!(
            parse(tokens),
            Ok(AST::Sequence(vec![cmd(&["a"]), cmd(&["b"]), cmd(&["c"])]))
        );
    }
}

mod boolean {
    use super::*;

    #[test]
    fn and() {
        let tokens = vec![w("cmd1"), Token::And, w("cmd2")];
        assert_eq!(
            parse(tokens),
            Ok(AST::And(Box::new(cmd(&["cmd1"])), Box::new(cmd(&["cmd2"]))))
        );
    }

    #[test]
    fn or() {
        let tokens = vec![w("cmd1"), Token::Or, w("cmd2")];
        assert_eq!(
            parse(tokens),
            Ok(AST::Or(Box::new(cmd(&["cmd1"])), Box::new(cmd(&["cmd2"]))))
        );
    }

    #[test]
    fn pipe_binds_tighter_than_and() {
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
}

mod background {
    use super::*;

    #[test]
    fn single_command() {
        let tokens = vec![w("sleep"), w("10"), Token::Background];
        assert_eq!(
            parse(tokens),
            Ok(AST::Background(Box::new(cmd(&["sleep", "10"]))))
        );
    }

    #[test]
    fn wraps_full_pipeline() {
        // a | b &  →  Background(Pipeline([a,b]))
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
    fn wraps_full_and_list() {
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
    fn followed_by_command_makes_sequence() {
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
    fn multiple_are_flat_sequence() {
        // a & b & c  →  Sequence([Background(a), Background(b), c])
        let tokens = vec![w("a"), Token::Background, w("b"), Token::Background, w("c")];
        assert_eq!(
            parse(tokens),
            Ok(AST::Sequence(vec![
                AST::Background(Box::new(cmd(&["a"]))),
                AST::Background(Box::new(cmd(&["b"]))),
                cmd(&["c"])
            ]))
        );
    }
}

mod subshell {
    use super::*;

    #[test]
    fn simple() {
        let tokens = vec![Token::LParen, w("echo"), w("hi"), Token::RParen];
        assert_eq!(
            parse(tokens),
            Ok(AST::Subshell(Box::new(cmd(&["echo", "hi"]))))
        );
    }

    #[test]
    fn with_pipeline() {
        let tokens = vec![Token::LParen, w("a"), Token::Pipe, w("b"), Token::RParen];
        assert_eq!(
            parse(tokens),
            Ok(AST::Subshell(Box::new(AST::Pipeline(vec![
                cmd(&["a"]),
                cmd(&["b"])
            ]))))
        );
    }
}

mod redirects {
    use super::*;

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
            Ok(cmd_r(
                &["cmd"],
                vec![Redirect::Out(segs("err.txt"), Some(2))]
            ))
        );
    }

    #[test]
    fn redirect_append() {
        let tokens = vec![w("echo"), w("hi"), Token::RedirectAppend(None), w("log.txt")];
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

    #[test]
    fn missing_target_is_error() {
        assert_eq!(
            parse(vec![w("cat"), Token::RedirectIn]),
            Err(ParserError::UnexpectedEnd)
        );
    }
}

mod errors {
    use super::*;

    #[test]
    fn unmatched_rparen() {
        assert_eq!(parse(vec![Token::RParen]), Err(ParserError::UnmatchedParen));
    }

    #[test]
    fn unmatched_lparen_extra_rparen() {
        assert_eq!(
            parse(vec![Token::LParen, w("echo"), Token::RParen, Token::RParen]),
            Err(ParserError::UnmatchedParen)
        );
    }
}
