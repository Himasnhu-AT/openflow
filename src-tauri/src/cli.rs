//! Command-line tools (`--selftest`, `--eval`, `--polishtest`, ...), run before the GUI starts.
//! Returns `Some(exit_code)` when a tool ran.

pub fn dispatch(_args: &[String]) -> Option<i32> {
    None
}
