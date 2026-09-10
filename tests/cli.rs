//! Exercises the binary without touching the system clipboard, so these run
//! on headless CI machines.

use std::process::{Command, Output};

fn cb(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cb"))
        .args(args)
        // Keep tests out of the real clipboard history.
        .env("CB_HISTORY_FILE", "")
        .output()
        .expect("failed to run cb")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn help_prints_usage() {
    for flag in ["-h", "--help"] {
        let output = cb(&[flag]);
        assert!(output.status.success(), "{}", stderr(&output));
        assert!(stdout(&output).starts_with("Usage: cb [FILE]"));
    }
}

#[test]
fn help_mentions_peek() {
    let output = cb(&["--help"]);
    assert!(stdout(&output).contains("cb peek"));
}

#[test]
fn peek_needs_a_terminal() {
    // `output()` gives the child a null stdin and piped stdout.
    let output = cb(&["peek"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(stderr(&output), "cb: peek needs an interactive terminal\n");
}

#[test]
fn peek_takes_no_arguments() {
    let output = cb(&["peek", "1"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("unexpected argument '1'"));
}

#[test]
fn help_mentions_watch() {
    let output = cb(&["--help"]);
    assert!(stdout(&output).contains("cb watch [--install | --uninstall]"));
}

#[test]
fn watch_needs_history() {
    // `cb()` turns history off.
    let output = cb(&["watch"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        stderr(&output),
        "cb: clipboard history is turned off because CB_HISTORY_FILE is empty\n"
    );
}

#[test]
fn watch_rejects_unknown_options() {
    let output = cb(&["watch", "--bogus"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("unknown option '--bogus'"));
}

#[test]
fn version_prints_the_crate_version() {
    let output = cb(&["--version"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        format!("cb {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn too_many_arguments_is_a_usage_error() {
    let output = cb(&["a", "b"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("unexpected argument 'b'"));
}

#[test]
fn unknown_option_is_a_usage_error() {
    let output = cb(&["--bogus"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("unknown option '--bogus'"));
}

#[test]
fn missing_file_names_the_path_instead_of_panicking() {
    let output = cb(&["does-not-exist.txt"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = stderr(&output);
    assert!(stderr.starts_with("cb: does-not-exist.txt: "), "{}", stderr);
    assert!(!stderr.contains("panicked"), "{}", stderr);
}
