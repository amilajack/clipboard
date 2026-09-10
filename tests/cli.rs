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
fn help_mentions_completions() {
    let output = cb(&["--help"]);
    assert!(stdout(&output).contains("cb completions SHELL"));
}

#[test]
fn completions_prints_the_script_for_each_shell() {
    for (shell, script) in [
        ("bash", include_str!("../completions/cb.bash")),
        ("zsh", include_str!("../completions/_cb")),
        ("fish", include_str!("../completions/cb.fish")),
    ] {
        let output = cb(&["completions", shell]);
        assert!(output.status.success(), "{}", stderr(&output));
        assert_eq!(stdout(&output), script);
    }
}

#[test]
fn completions_for_an_unknown_shell_is_a_usage_error() {
    let output = cb(&["completions", "tcsh"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("unknown shell 'tcsh'"));
}

/// Runs the bash completion script the way bash does on Tab.
#[cfg(unix)]
mod bash_completion {
    use super::*;
    use std::fs;
    use std::path::Path;

    /// Completes the last of `words`, typed after `cb`, in a directory holding
    /// `notes.txt` and `src/`.
    fn complete(words: &[&str]) -> Vec<String> {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("bash-completion");
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("notes.txt"), "").unwrap();

        let script = stdout(&cb(&["completions", "bash"]));
        let driver = r#"
COMP_WORDS=(cb "$@")
COMP_CWORD=$#
_cb cb "${COMP_WORDS[COMP_CWORD]}" "${COMP_WORDS[COMP_CWORD-1]}"
printf '%s\n' "${COMPREPLY[@]}"
"#;
        let output = Command::new("bash")
            .arg("-c")
            .arg(script + driver)
            .arg("bash")
            .args(words)
            .current_dir(&dir)
            .output()
            .expect("failed to run bash");
        assert!(output.status.success(), "{}", stderr(&output));

        let mut completions: Vec<String> = stdout(&output)
            .lines()
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect();
        completions.sort();
        completions
    }

    #[test]
    fn first_argument_is_a_command_or_a_file() {
        assert_eq!(
            complete(&[""]),
            ["completions", "notes.txt", "peek", "src", "watch"]
        );
        assert_eq!(complete(&["p"]), ["peek"]);
        assert_eq!(complete(&["no"]), ["notes.txt"]);
    }

    #[test]
    fn a_dash_completes_options() {
        assert_eq!(complete(&["-"]), ["--help", "--version", "-V", "-h"]);
        assert_eq!(complete(&["--v"]), ["--version"]);
    }

    #[test]
    fn completions_is_followed_by_a_shell() {
        assert_eq!(complete(&["completions", ""]), ["bash", "fish", "zsh"]);
        assert_eq!(complete(&["completions", "z"]), ["zsh"]);
    }

    #[test]
    fn watch_is_followed_by_its_options() {
        assert_eq!(complete(&["watch", ""]), ["--install", "--uninstall"]);
        assert_eq!(complete(&["watch", "--u"]), ["--uninstall"]);
    }

    #[test]
    fn nothing_follows_a_complete_command_line() {
        assert!(complete(&["peek", ""]).is_empty());
        assert!(complete(&["notes.txt", ""]).is_empty());
        assert!(complete(&["completions", "zsh", ""]).is_empty());
        assert!(complete(&["watch", "--install", ""]).is_empty());
    }
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
