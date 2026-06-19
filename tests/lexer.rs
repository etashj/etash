#[cfg(test)]
mod tests {
    use Token::*;
    use WordSegment::*;
    use etash::lexer::*;

    /// Feeds `lines` through `tokenize` one at a time, threading partial
    /// state the same way main.rs does, and returns the final result.
    /// Panics if input runs out while still Open (test bug, not lexer bug).
    fn tokenize_lines(lines: &[&str]) -> (Vec<Token>, InputState) {
        let mut existing = None;
        let mut partial = PartialState::None;
        let mut iter = lines.iter();

        let first = iter.next().expect("need at least one line");
        let (mut tokens, mut status) = tokenize(first, &mut existing, &mut partial);

        for line in iter {
            if !matches!(status, InputState::Open(_)) {
                panic!("tokenize closed early, but more lines were supplied");
            }
            existing = Some(tokens);
            (tokens, status) = tokenize(line, &mut existing, &mut partial);
        }
        (tokens, status)
    }

    /// Convenience for the common single-line case.
    fn tokenize_line(line: &str) -> (Vec<Token>, InputState) {
        tokenize_lines(&[line])
    }

    // ---- bare words ----

    #[test]
    fn single_word() {
        let (tokens, status) = tokenize_line("echo");
        assert_eq!(status, InputState::Closed);
        assert_eq!(tokens, vec![Word(vec![Expandable("echo".into())])]);
    }

    #[test]
    fn multiple_words() {
        let (tokens, status) = tokenize_line("echo hi there");
        assert_eq!(status, InputState::Closed);
        assert_eq!(
            tokens,
            vec![
                Word(vec![Expandable("echo".into())]),
                Word(vec![Expandable("hi".into())]),
                Word(vec![Expandable("there".into())]),
            ]
        );
    }

    // ---- metacharacters ----

    #[test]
    fn pipe_and_or() {
        let (tokens, _) = tokenize_line("a | b || c");
        assert_eq!(
            tokens,
            vec![
                Word(vec![Expandable("a".into())]),
                Token::Pipe,
                Word(vec![Expandable("b".into())]),
                Token::Or,
                Word(vec![Expandable("c".into())]),
            ]
        );
    }

    #[test]
    fn redirects() {
        let (tokens, _) = tokenize_line("a > b >> c < d");
        assert_eq!(
            tokens,
            vec![
                Word(vec![Expandable("a".into())]),
                Token::RedirectOut(None),
                Word(vec![Expandable("b".into())]),
                Token::RedirectAppend,
                Word(vec![Expandable("c".into())]),
                Token::RedirectIn,
                Word(vec![Expandable("d".into())]),
            ]
        );
    }

    // ---- quotes ----

    #[test]
    fn single_quote_literal() {
        let (tokens, status) = tokenize_line("'hello there'");
        assert_eq!(status, InputState::Closed);
        assert_eq!(tokens, vec![Word(vec![Literal("hello there".into())])]);
    }

    #[test]
    fn double_quote_expandable() {
        let (tokens, status) = tokenize_line("\"hello there\"");
        assert_eq!(status, InputState::Closed);
        assert_eq!(tokens, vec![Word(vec![Expandable("hello there".into())])]);
    }

    #[test]
    fn quote_embedded_in_word_joins_one_token() {
        // echo "hellot there"  should be: [echo] ["hellot there"]
        let (tokens, _) = tokenize_line(r#"echo "hellot there""#);
        assert_eq!(
            tokens,
            vec![
                Word(vec![Expandable("echo".into())]),
                Word(vec![Expandable("hellot there".into())]),
            ]
        );
    }

    #[test]
    fn word_prefixed_quote_joins_into_one_word() {
        // abc"def" -> single word, not two
        let (tokens, _) = tokenize_line(r#"abc"def""#);
        assert_eq!(
            tokens,
            vec![Word(vec![
                Expandable("abc".into()),
                Expandable("def".into())
            ])]
        );
    }

    // ---- escapes ----

    #[test]
    fn backslash_escapes_space_joins_words() {
        let (tokens, _) = tokenize_line(r"foo\ bar");
        assert_eq!(
            tokens,
            vec![Word(vec![
                Expandable("foo".into()),
                Literal(" ".into()),
                Expandable("bar".into()),
            ])]
        );
    }

    #[test]
    fn double_quote_escape_sequences() {
        let (tokens, _) = tokenize_line(r#""a\"b\\c\$d""#);
        // \" -> ", \\ -> \, \$ -> $, rest literal text stays Expandable
        assert_eq!(
            tokens,
            vec![Word(vec![
                Expandable("a".into()),
                Literal("\"".into()),
                Expandable("b".into()),
                Literal("\\".into()),
                Expandable("c".into()),
                Literal("$".into()),
                Expandable("d".into()),
            ])]
        );
    }

    // ---- multi-line resumption ----

    #[test]
    fn double_quote_spans_lines() {
        let (tokens, status) = tokenize_lines(&["\"hello\n", "there\""]);
        assert_eq!(status, InputState::Closed);
        assert_eq!(tokens, vec![Word(vec![Expandable("hello\nthere".into())])]);
    }

    #[test]
    fn single_quote_spans_lines() {
        let (tokens, status) = tokenize_lines(&["'hello\n", "there'"]);
        assert_eq!(status, InputState::Closed);
        assert_eq!(tokens, vec![Word(vec![Literal("hello\nthere".into())])]);
    }

    #[test]
    fn trailing_backslash_continues_word_across_lines() {
        // foo\<newline>bar should join into "foobar" (no embedded newline)
        let (tokens, status) = tokenize_lines(&["foo\\\n", "bar"]);
        assert_eq!(status, InputState::Closed);
        assert_eq!(tokens, vec![Word(vec![Expandable("foobar".into())])]);
    }

    #[test]
    fn unterminated_double_quote_reports_open() {
        let (_, status) = tokenize_line("\"hello");
        assert_eq!(status, InputState::Open(Openable::Dquote));
    }

    #[test]
    fn unterminated_single_quote_reports_open() {
        let (_, status) = tokenize_line("'hello");
        assert_eq!(status, InputState::Open(Openable::Quote));
    }
}
