use std::collections::{HashMap, HashSet};

use crate::lexer::WordSegment;

/// Central shell state: variables, exports, and last exit status.
///
/// `Shell::new()` seeds all variables from the process environment and
/// marks them exported so child processes inherit them. `load_rc` reads
/// `~/.etashrc` for simple `KEY=value` / `export KEY=value` assignments;
/// full rc execution (functions, conditionals, etc.) requires the execution
/// layer and will replace this once that exists.
pub struct Shell {
    vars: HashMap<String, String>,
    exported: HashSet<String>,
    /// Exit status of the last foreground command ($?).
    pub status: i32,
}

impl Default for Shell {
    fn default() -> Self {
        Self::new()
    }
}

impl Shell {
    /// Creates a Shell pre-seeded from the current process environment.
    pub fn new() -> Self {
        let mut shell = Shell {
            vars: HashMap::new(),
            exported: HashSet::new(),
            status: 0,
        };
        for (k, v) in std::env::vars() {
            shell.exported.insert(k.clone());
            shell.vars.insert(k, v);
        }
        shell
    }

    /// Reads `~/.etashrc` and applies `KEY=value` / `export KEY=value` lines.
    /// Lines starting with `#` and blank lines are skipped.
    /// Silently returns if the file does not exist.
    pub fn load_rc(&mut self) {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let rc_path = std::path::Path::new(&home).join(".etashrc");
        let Ok(content) = std::fs::read_to_string(rc_path) else {
            return;
        };
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (do_export, assignment) = if let Some(rest) = line.strip_prefix("export ") {
                (true, rest.trim())
            } else {
                (false, line)
            };
            if let Some((key, val)) = assignment.split_once('=') {
                let key = key.trim().to_string();
                self.vars.insert(key.clone(), val.to_string());
                if do_export {
                    self.exported.insert(key);
                }
            }
        }
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.vars.get(name).map(String::as_str)
    }

    pub fn set(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.vars.insert(name.into(), value.into());
    }

    /// Marks an existing variable as exported without changing its value.
    pub fn export(&mut self, name: impl Into<String>) {
        self.exported.insert(name.into());
    }

    /// Sets a variable and marks it exported in one step.
    pub fn set_exported(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        self.vars.insert(name.clone(), value.into());
        self.exported.insert(name);
    }

    pub fn unset(&mut self, name: &str) {
        self.vars.remove(name);
        self.exported.remove(name);
    }

    pub fn is_exported(&self, name: &str) -> bool {
        self.exported.contains(name)
    }

    /// Returns a snapshot of all exported variables for passing to child processes.
    pub fn env_for_child(&self) -> HashMap<String, String> {
        self.exported
            .iter()
            .filter_map(|k| self.vars.get(k).map(|v| (k.clone(), v.clone())))
            .collect()
    }

    /// Expands a word (a sequence of `Literal`/`Expandable` segments) to a
    /// concrete string using the current variable state.
    pub fn expand_word(&self, segs: &[WordSegment]) -> String {
        let mut result = String::new();
        for seg in segs {
            match seg {
                WordSegment::Literal(s) => result.push_str(s),
                WordSegment::Expandable(s) => result.push_str(&self.expand_str(s)),
            }
        }
        result
    }

    /// Expands `$VAR`, `${VAR}`, `$?`, and `$$` within a raw segment string.
    /// Undefined variables expand to empty string. A bare `$` with no valid
    /// name following is kept as a literal `$`.
    fn expand_str(&self, s: &str) -> String {
        let mut res = String::new();
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '$' {
                res.push(c);
                continue;
            }
            match chars.peek().copied() {
                Some('?') => {
                    chars.next();
                    res.push_str(&self.status.to_string());
                }
                Some('$') => {
                    chars.next();
                    res.push_str(&std::process::id().to_string());
                }
                Some('{') => {
                    chars.next();
                    let name = consume_braced(&mut chars);
                    res.push_str(self.get(&name).unwrap_or(""));
                }
                Some(nc) if nc.is_alphabetic() || nc == '_' => {
                    let name = consume_plain(&mut chars);
                    res.push_str(self.get(&name).unwrap_or(""));
                }
                _ => res.push('$'),
            }
        }
        res
    }
}

fn consume_braced(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut name = String::new();
    for c in chars.by_ref() {
        if c == '}' {
            break;
        }
        name.push(c);
    }
    name
}

fn consume_plain(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut name = String::new();
    while let Some(&c) = chars.peek() {
        if c.is_alphanumeric() || c == '_' {
            name.push(c);
            chars.next();
        } else {
            break;
        }
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::WordSegment::{Expandable, Literal};

    fn lit(s: &str) -> WordSegment {
        Literal(s.to_string())
    }
    fn exp(s: &str) -> WordSegment {
        Expandable(s.to_string())
    }

    fn shell_with(vars: &[(&str, &str)]) -> Shell {
        let mut s = Shell {
            vars: HashMap::new(),
            exported: HashSet::new(),
            status: 0,
        };
        for (k, v) in vars {
            s.vars.insert(k.to_string(), v.to_string());
        }
        s
    }

    #[test]
    fn get_set_unset() {
        let mut s = shell_with(&[]);
        assert_eq!(s.get("FOO"), None);
        s.set("FOO", "bar");
        assert_eq!(s.get("FOO"), Some("bar"));
        s.unset("FOO");
        assert_eq!(s.get("FOO"), None);
    }

    #[test]
    fn export_and_env_for_child() {
        let mut s = shell_with(&[("A", "1"), ("B", "2")]);
        s.export("A");
        let env = s.env_for_child();
        assert_eq!(env.get("A").map(String::as_str), Some("1"));
        assert!(!env.contains_key("B"));
    }

    #[test]
    fn set_exported_appears_in_child_env() {
        let mut s = shell_with(&[]);
        s.set_exported("PATH", "/usr/bin");
        assert!(s.is_exported("PATH"));
        assert_eq!(s.env_for_child().get("PATH").map(String::as_str), Some("/usr/bin"));
    }

    #[test]
    fn unset_removes_from_exports() {
        let mut s = shell_with(&[]);
        s.set_exported("X", "1");
        s.unset("X");
        assert!(!s.is_exported("X"));
        assert!(s.env_for_child().is_empty());
    }

    // --- expand_word ---

    #[test]
    fn expand_literal_passthrough() {
        let s = shell_with(&[]);
        assert_eq!(s.expand_word(&[lit("hello world")]), "hello world");
    }

    #[test]
    fn expand_plain_var() {
        let s = shell_with(&[("HOME", "/home/user")]);
        assert_eq!(s.expand_word(&[exp("$HOME/bin")]), "/home/user/bin");
    }

    #[test]
    fn expand_braced_var() {
        let s = shell_with(&[("NAME", "etash")]);
        assert_eq!(s.expand_word(&[exp("${NAME}_rc")]), "etash_rc");
    }

    #[test]
    fn expand_undefined_var_is_empty() {
        let s = shell_with(&[]);
        assert_eq!(s.expand_word(&[exp("$UNDEFINED")]), "");
    }

    #[test]
    fn expand_bare_dollar_is_literal() {
        let s = shell_with(&[]);
        assert_eq!(s.expand_word(&[exp("$ ")]), "$ ");
    }

    #[test]
    fn expand_status_var() {
        let mut s = shell_with(&[]);
        s.status = 42;
        assert_eq!(s.expand_word(&[exp("$?")]), "42");
    }

    #[test]
    fn expand_mixed_segments() {
        let s = shell_with(&[("USER", "alice")]);
        // "Hello, " + $USER + "!"  (literal + expandable + literal)
        assert_eq!(
            s.expand_word(&[lit("Hello, "), exp("$USER"), lit("!")]),
            "Hello, alice!"
        );
    }
}
