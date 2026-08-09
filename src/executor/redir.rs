extern crate libc;
use std::fs::{File, OpenOptions};
use std::io;
use std::process::Stdio;

use crate::parser::ast::Redirect;
use crate::shell::Shell;

pub fn apply_redirects(
    cmd: &mut std::process::Command,
    redirs: &[Redirect],
    shell: &Shell,
) -> io::Result<()> {
    let mut dups: Vec<(i32, i32)> = Vec::new();

    for redir in redirs {
        match redir {
            Redirect::In(segs) => {
                let path = shell.expand_word(segs);
                let file = File::open(&path)?;
                cmd.stdin(Stdio::from(file));
            }
            Redirect::Out(segs, fd) => {
                let path = shell.expand_word(segs);
                let file = File::create(&path)?;
                match fd.unwrap_or(1) {
                    1 => { cmd.stdout(Stdio::from(file)); }
                    2 => { cmd.stderr(Stdio::from(file)); }
                    n => { dups.push((1, n as i32)); } // open as stdout then dup to target
                }
            }
            Redirect::Append(segs, fd) => {
                let path = shell.expand_word(segs);
                let file = OpenOptions::new().append(true).create(true).open(&path)?;
                match fd.unwrap_or(1) {
                    1 => { cmd.stdout(Stdio::from(file)); }
                    2 => { cmd.stderr(Stdio::from(file)); }
                    _ => { cmd.stdout(Stdio::from(file)); }
                }
            }
            Redirect::Dup { target, source } => {
                dups.push((*source as i32, *target as i32));
            }
        }
    }

    if !dups.is_empty() {
        unsafe {
            use std::os::unix::process::CommandExt;
            cmd.pre_exec(move || {
                for (source, target) in &dups {
                    if libc::dup2(*source, *target) == -1 {
                        return Err(io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
    }

    Ok(())
}
