use crate::shell::Shell;

/// Runs a builtin command if `argv[0]` names one. Returns `Some(exit_status)`
/// if handled, `None` if the command should be executed as an external process.
pub fn run_builtin(_argv: &[String], _shell: &mut Shell) -> Option<i32> {
    todo!()
}
