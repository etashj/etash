extern crate libc;

use etash::executor::{execute, reap_jobs};
use etash::lexer::{
    InputState::Open,
    Openable::{CmdAnd, CmdOr, Dquote, Quote, Word},
    PartialState, tokenize,
};
use etash::parser::parse;
use etash::shell::Shell;

use rustyline::completion::{Completer, FilenameCompleter, Pair};
use rustyline::error::ReadlineError;
use rustyline::hint::HistoryHinter;
use rustyline::{CompletionType, Config, Context, Editor};
use rustyline_derive::{Helper, Highlighter, Hinter, Validator};

use std::env;
use std::os::unix::fs::PermissionsExt;

// ── completer ────────────────────────────────────────────────────────────────

#[derive(Helper, Highlighter, Hinter, Validator)]
struct EtashHelper {
    file_completer: FilenameCompleter,
    builtins: Vec<String>,
    #[rustyline(Hinter)]
    hinter: HistoryHinter,
}

impl EtashHelper {
    fn new() -> Self {
        Self {
            file_completer: FilenameCompleter::new(),
            builtins: vec![
                "cd", "exit", "export", "unset", "pwd", "fg", "bg",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            hinter: HistoryHinter::new(),
        }
    }

    fn path_commands(&self) -> Vec<String> {
        let path = env::var("PATH").unwrap_or_default();
        let mut cmds = self.builtins.clone();
        for dir in path.split(':') {
            let Ok(entries) = std::fs::read_dir(dir) else { continue };
            for entry in entries.flatten() {
                let Ok(meta) = entry.metadata() else { continue };
                if meta.permissions().mode() & 0o111 != 0 {
                    if let Some(name) = entry.file_name().to_str().map(String::from) {
                        cmds.push(name);
                    }
                }
            }
        }
        cmds.sort();
        cmds.dedup();
        cmds
    }
}

impl Completer for EtashHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let word_start = line[..pos]
            .rfind(|c: char| c.is_whitespace())
            .map(|i| i + 1)
            .unwrap_or(0);
        let word = &line[word_start..pos];
        let is_first_word = line[..word_start].trim().is_empty();

        if is_first_word {
            let candidates = self
                .path_commands()
                .into_iter()
                .filter(|c| c.starts_with(word))
                .map(|c| Pair { display: c.clone(), replacement: c })
                .collect();
            Ok((word_start, candidates))
        } else {
            self.file_completer.complete(line, pos, ctx)
        }
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn format_cwd(shell: &Shell) -> String {
    let cwd = shell.cwd.to_string_lossy();
    if let Some(home) = shell.get("HOME") {
        if let Some(rest) = cwd.strip_prefix(home) {
            return format!("~{rest}");
        }
    }
    cwd.into_owned()
}

fn history_path() -> String {
    env::var("HOME")
        .map(|h| format!("{h}/.etash_history"))
        .unwrap_or_else(|_| ".etash_history".into())
}

// ── main ──────────────────────────────────────────────────────────────────────

fn main() {
    unsafe {
        libc::signal(libc::SIGINT, libc::SIG_IGN);
        libc::signal(libc::SIGQUIT, libc::SIG_IGN);
        libc::signal(libc::SIGTSTP, libc::SIG_IGN);
    }

    let mut shell = Shell::new();
    shell.load_rc();

    let config = Config::builder()
        .completion_type(CompletionType::List)
        .build();

    let mut rl: Editor<EtashHelper, _> = Editor::with_config(config).expect("failed to create editor");
    rl.set_helper(Some(EtashHelper::new()));
    rl.load_history(&history_path()).ok();

    loop {
        reap_jobs(&mut shell);

        let prompt = format!("{} ? ", format_cwd(&shell));

        let mut line = match rl.readline(&prompt) {
            Ok(l) if l.trim().is_empty() => continue,
            Ok(l) => { rl.add_history_entry(&l).ok(); l }
            Err(ReadlineError::Interrupted) => continue,
            Err(ReadlineError::Eof) => break,
            Err(e) => { eprintln!("readline: {e}"); break; }
        };

        line.push('\n');

        let mut existing = None;
        let mut partial = PartialState::None;
        let (mut tokens, mut status) = tokenize(&line, &mut existing, &mut partial);

        while let Open(reason) = &status {
            let cont_prompt = match reason {
                Quote  => "quote> ",
                Dquote => "dquote> ",
                Word   => "> ",
                CmdAnd => "cmdand> ",
                CmdOr  => "cmdor> ",
            };
            let mut next = match rl.readline(cont_prompt) {
                Ok(l) => l,
                Err(ReadlineError::Eof | ReadlineError::Interrupted) => break,
                Err(e) => { eprintln!("readline: {e}"); break; }
            };
            next.push('\n');
            existing = Some(tokens);
            (tokens, status) = tokenize(&next, &mut existing, &mut partial);
        }

        match parse(tokens) {
            Ok(ast) => {
                let code = execute(&ast, &mut shell);
                shell.status = code;
            }
            Err(e) => eprintln!("parse error: {e:?}"),
        }
    }

    rl.save_history(&history_path()).ok();
}
