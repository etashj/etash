use std::fs::{File, OpenOptions};
use std::process::Stdio;

use crate::parser::ast::Redirect;
use crate::shell::Shell;

pub fn apply_redirects(
    cmd: &mut std::process::Command,
    redirs: &[Redirect],
    shell: &Shell,
) -> std::io::Result<()> {
    for redir in redirs {
        match redir {
            Redirect::In(segs) => {
                let path = shell.expand_word(segs);
                let file = File::open(&path)?;
                cmd.stdin(Stdio::from(file));
            }
            Redirect::Out(segs, _fd) => {
                let path = shell.expand_word(segs);
                let file = File::create(&path)?;
                cmd.stdout(Stdio::from(file));
            }
            Redirect::Append(segs, _fd) => {
                let path = shell.expand_word(segs);
                let file = OpenOptions::new().append(true).create(true).open(&path)?;
                cmd.stdout(Stdio::from(file));
            }
        }
    }
    Ok(())
}
