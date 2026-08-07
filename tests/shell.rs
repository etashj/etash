use etash::lexer::WordSegment::{Expandable, Literal};
use etash::lexer::WordSegment;
use etash::shell::Shell;

fn lit(s: &str) -> WordSegment {
    Literal(s.to_string())
}

fn exp(s: &str) -> WordSegment {
    Expandable(s.to_string())
}

mod vars {
    use super::*;

    #[test]
    fn get_set_unset() {
        let mut s = Shell::from_vars(&[]);
        assert_eq!(s.get("FOO"), None);
        s.set("FOO", "bar");
        assert_eq!(s.get("FOO"), Some("bar"));
        s.unset("FOO");
        assert_eq!(s.get("FOO"), None);
    }

    #[test]
    fn export_filters_env_for_child() {
        let mut s = Shell::from_vars(&[("A", "1"), ("B", "2")]);
        s.export("A");
        let env = s.env_for_child();
        assert_eq!(env.get("A").map(String::as_str), Some("1"));
        assert!(!env.contains_key("B"));
    }

    #[test]
    fn set_exported_appears_in_child_env() {
        let mut s = Shell::from_vars(&[]);
        s.set_exported("PATH", "/usr/bin");
        assert!(s.is_exported("PATH"));
        assert_eq!(s.env_for_child().get("PATH").map(String::as_str), Some("/usr/bin"));
    }

    #[test]
    fn unset_removes_from_exports() {
        let mut s = Shell::from_vars(&[]);
        s.set_exported("X", "1");
        s.unset("X");
        assert!(!s.is_exported("X"));
        assert!(s.env_for_child().is_empty());
    }
}

mod expand {
    use super::*;

    #[test]
    fn literal_passthrough() {
        let s = Shell::from_vars(&[]);
        assert_eq!(s.expand_word(&[lit("hello world")]), "hello world");
    }

    #[test]
    fn plain_var() {
        let s = Shell::from_vars(&[("HOME", "/home/user")]);
        assert_eq!(s.expand_word(&[exp("$HOME/bin")]), "/home/user/bin");
    }

    #[test]
    fn braced_var() {
        let s = Shell::from_vars(&[("NAME", "etash")]);
        assert_eq!(s.expand_word(&[exp("${NAME}_rc")]), "etash_rc");
    }

    #[test]
    fn undefined_var_is_empty() {
        let s = Shell::from_vars(&[]);
        assert_eq!(s.expand_word(&[exp("$UNDEFINED")]), "");
    }

    #[test]
    fn bare_dollar_is_literal() {
        let s = Shell::from_vars(&[]);
        assert_eq!(s.expand_word(&[exp("$ ")]), "$ ");
    }

    #[test]
    fn status_var() {
        let mut s = Shell::from_vars(&[]);
        s.status = 42;
        assert_eq!(s.expand_word(&[exp("$?")]), "42");
    }

    #[test]
    fn mixed_segments() {
        let s = Shell::from_vars(&[("USER", "alice")]);
        assert_eq!(
            s.expand_word(&[lit("Hello, "), exp("$USER"), lit("!")]),
            "Hello, alice!"
        );
    }
}
