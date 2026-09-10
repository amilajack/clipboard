use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use arboard::Clipboard;

use crate::completions::Shell;
use crate::highlight::Highlighter;
use crate::history::Use;

mod completions;
mod highlight;
mod history;
mod peek;
mod platform;
mod watch;

const USAGE: &str = "\
Usage: cb [FILE]
       cb peek
       cb watch [--install | --uninstall]
       cb completions SHELL

Copy FILE, or whatever is piped in, to the system clipboard.
With no FILE and nothing piped in, print the clipboard.

Commands:
  peek               Search clipboard history and copy an entry again
  watch              Record everything copied, in any program
    --install        Also start watching whenever you log in
    --uninstall      Stop watching, now and at login
  completions SHELL  Print tab completion for bash, zsh or fish

Examples:
  cb package.json    Copy a file
  git diff | cb      Copy piped input
  cb | grep hello    Search the clipboard
  cb peek            Browse what you copied before
  cb watch --install Keep history of every copy, in any program

Options:
  -h, --help         Print help
  -V, --version      Print version
";

/// Set in the environment of the background process that serves the clipboard.
#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
))]
const SERVE_ENV: &str = "CB_SERVE_CLIPBOARD";

#[derive(Debug, PartialEq, Eq)]
enum Action {
    Help,
    Version,
    Print,
    CopyFile(PathBuf),
    CopyStdin,
    Peek,
    Watch,
    WatchInstall,
    WatchUninstall,
    Completions(Shell),
}

/// Decides what to do from the arguments (excluding the program name).
///
/// An explicit file always wins over stdin, so `cb notes.txt` behaves the same
/// in a script or IDE task, where stdin is not a terminal, as it does at a prompt.
/// `peek`, `watch` and `completions` are commands, so files by those names are
/// copied as `cb ./peek`, `cb ./watch` and `cb ./completions`.
fn parse_args<I>(args: I, stdin_is_terminal: bool) -> Result<Action, String>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args = args.into_iter();
    let arg = match args.next() {
        Some(arg) => arg,
        None if stdin_is_terminal => return Ok(Action::Print),
        None => return Ok(Action::CopyStdin),
    };
    let action = match arg.to_str() {
        Some("-h" | "--help") => Action::Help,
        Some("-V" | "--version") => Action::Version,
        Some("peek") => Action::Peek,
        Some("watch") => match args.next() {
            None => Action::Watch,
            Some(option) => match option.to_str() {
                Some("--install") => Action::WatchInstall,
                Some("--uninstall") => Action::WatchUninstall,
                Some(flag) if flag.starts_with('-') => {
                    return Err(format!("unknown option '{}'", flag));
                }
                _ => {
                    return Err(format!(
                        "unexpected argument '{}'",
                        option.to_string_lossy()
                    ));
                }
            },
        },
        Some("completions") => {
            let name = args
                .next()
                .ok_or_else(|| format!("completions needs a shell: {}", completions::SHELLS))?;
            let shell = name.to_str().and_then(Shell::from_name).ok_or_else(|| {
                format!(
                    "unknown shell '{}'; expected {}",
                    name.to_string_lossy(),
                    completions::SHELLS
                )
            })?;
            Action::Completions(shell)
        }
        Some(flag) if flag.len() > 1 && flag.starts_with('-') => {
            return Err(format!("unknown option '{}'", flag));
        }
        _ => Action::CopyFile(PathBuf::from(arg)),
    };
    if let Some(extra) = args.next() {
        return Err(format!("unexpected argument '{}'", extra.to_string_lossy()));
    }
    Ok(action)
}

/// Drops one trailing line ending.
///
/// Commands like `echo` end their output with a newline nobody means to paste.
/// `print` adds it back, so `cb < a; cb > b` leaves `b` identical to `a`.
/// Everything else, including leading whitespace, is part of the content.
fn strip_trailing_newline(text: &str) -> &str {
    text.strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
        .unwrap_or(text)
}

fn run(action: Action) -> Result<(), String> {
    match action {
        Action::Help => {
            print!("{}", USAGE);
            Ok(())
        }
        Action::Version => {
            println!("cb {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Action::Print => print(),
        Action::CopyFile(path) => {
            let text =
                fs::read_to_string(&path).map_err(|e| format!("{}: {}", path.display(), e))?;
            let text = strip_trailing_newline(&text);
            copy(text)?;
            remember(text, Some(&path), Use::Copied);
            Ok(())
        }
        Action::CopyStdin => {
            let mut text = String::new();
            io::stdin()
                .read_to_string(&mut text)
                .map_err(|e| format!("stdin: {}", e))?;
            let text = strip_trailing_newline(&text);
            copy(text)?;
            remember(text, None, Use::Copied);
            Ok(())
        }
        Action::Peek => peek(),
        Action::Watch => watch::run(),
        Action::WatchInstall => watch::install(),
        Action::WatchUninstall => watch::uninstall(),
        Action::Completions(shell) => {
            print!("{}", shell.script());
            Ok(())
        }
    }
}

/// Adds text to history. Failing to is worth a warning, not a failed copy.
fn remember(text: &str, source: Option<&Path>, how: Use) {
    if let Err(message) = history::record(text, source, how) {
        eprintln!("cb: warning: could not save history: {}", message);
    }
}

/// Adds text read off the clipboard to history, unless whoever copied it
/// asked for it not to be kept, as password managers do.
fn remember_clipboard(text: &str) {
    if !platform::is_concealed() {
        remember(text, None, Use::Seen);
    }
}

fn peek() -> Result<(), String> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err("peek needs an interactive terminal".to_owned());
    }
    let path = history::require_path()?;
    // Catch up on whatever was copied outside of cb since it last ran. There
    // may be no clipboard to read, as over SSH, and history works without one.
    if let Ok(text) = Clipboard::new().and_then(|mut clipboard| clipboard.get_text()) {
        remember_clipboard(&text);
    }
    let entries = history::load(&path).map_err(|e| format!("{}: {}", path.display(), e))?;
    if entries.is_empty() {
        return Err("clipboard history is empty; copy something with cb first".to_owned());
    }
    let highlighter = Highlighter::from_env()?;
    let Some(entry) = peek::run(entries, &highlighter).map_err(|e| e.to_string())? else {
        return Ok(());
    };
    copy(&entry.text)?;
    remember(&entry.text, entry.source.as_deref(), Use::Copied);
    Ok(())
}

fn print() -> Result<(), String> {
    let text = Clipboard::new()
        .and_then(|mut clipboard| clipboard.get_text())
        .map_err(|e| e.to_string())?;
    // Without `cb watch`, printing is how copies made outside of cb find their
    // way into history.
    remember_clipboard(&text);
    let mut stdout = io::stdout().lock();
    match writeln!(stdout, "{}", text).and_then(|()| stdout.flush()) {
        // The reader went away early, as with `cb | head -1`; that's not an error.
        Err(e) if e.kind() != io::ErrorKind::BrokenPipe => Err(e.to_string()),
        _ => Ok(()),
    }
}

#[cfg(not(all(
    unix,
    not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
)))]
fn copy(text: &str) -> Result<(), String> {
    Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_text(text))
        .map_err(|e| e.to_string())
}

/// On X11 and Wayland the copied text lives in the process that set it and
/// vanishes when that process exits. Hand it to a detached copy of ourselves
/// that keeps serving it until another program copies something.
#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
))]
fn copy(text: &str) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    let exe = env::current_exe().map_err(|e| e.to_string())?;
    // The server gets its own pipes rather than ours, so that `cb f | cat` and
    // `$(cb f)` finish immediately instead of waiting for it to exit. Its own
    // process group keeps a Ctrl-C at the prompt from reaching it.
    let mut child = Command::new(exe)
        .env(SERVE_ENV, "1")
        .current_dir("/")
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to start clipboard server: {}", e))?;

    let sent = child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(text.as_bytes());

    // One byte means the server connected and is taking over; EOF means it failed.
    let mut ready = [0; 1];
    let mut stdout = child.stdout.take().expect("stdout is piped");
    if sent.is_ok() && stdout.read_exact(&mut ready).is_ok() {
        return Ok(());
    }

    child.kill().ok();
    let mut message = String::new();
    child
        .stderr
        .take()
        .expect("stderr is piped")
        .read_to_string(&mut message)
        .ok();
    child.wait().ok();
    match (message.trim_end(), sent) {
        ("", Err(e)) => Err(format!("failed to start clipboard server: {}", e)),
        ("", Ok(())) => Err("clipboard server exited unexpectedly".to_owned()),
        (message, _) => Err(message.to_owned()),
    }
}

/// Runs in the background process started by `copy`.
#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
))]
fn serve() -> Result<(), String> {
    use arboard::SetExtLinux;

    let mut text = String::new();
    io::stdin()
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    let mut clipboard = Clipboard::new().map_err(|e| e.to_string())?;

    // Connecting to the display is what fails in practice (no X11 or Wayland
    // session, say over SSH), so that's the point to tell the parent it can exit.
    let mut stdout = io::stdout();
    stdout
        .write_all(b"\n")
        .and_then(|()| stdout.flush())
        .map_err(|e| e.to_string())?;

    // Blocks until another program takes over the clipboard.
    clipboard.set().wait().text(text).map_err(|e| e.to_string())
}

fn main() -> ExitCode {
    #[cfg(all(
        unix,
        not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
    ))]
    if env::var_os(SERVE_ENV).is_some() {
        return match serve() {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                // Read by the parent, which adds its own `cb:` prefix.
                eprintln!("{}", message);
                ExitCode::FAILURE
            }
        };
    }

    let action = match parse_args(env::args_os().skip(1), io::stdin().is_terminal()) {
        Ok(action) => action,
        Err(message) => {
            eprintln!("cb: {}\nTry 'cb --help' for more information.", message);
            return ExitCode::from(2);
        }
    };
    match run(action) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("cb: {}", message);
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str], stdin_is_terminal: bool) -> Result<Action, String> {
        parse_args(args.iter().map(OsString::from), stdin_is_terminal)
    }

    #[test]
    fn no_args_at_a_terminal_prints() {
        assert_eq!(parse(&[], true), Ok(Action::Print));
    }

    #[test]
    fn no_args_with_piped_input_copies_stdin() {
        assert_eq!(parse(&[], false), Ok(Action::CopyStdin));
    }

    #[test]
    fn file_argument_wins_over_piped_input() {
        let expected = Ok(Action::CopyFile(PathBuf::from("notes.txt")));
        assert_eq!(parse(&["notes.txt"], true), expected);
        assert_eq!(parse(&["notes.txt"], false), expected);
    }

    #[test]
    fn help_and_version_flags() {
        for flag in ["-h", "--help"] {
            assert_eq!(parse(&[flag], true), Ok(Action::Help));
        }
        for flag in ["-V", "--version"] {
            assert_eq!(parse(&[flag], true), Ok(Action::Version));
        }
    }

    #[test]
    fn peek_is_a_command() {
        assert_eq!(parse(&["peek"], true), Ok(Action::Peek));
        assert_eq!(parse(&["peek"], false), Ok(Action::Peek));
        assert_eq!(
            parse(&["./peek"], true),
            Ok(Action::CopyFile(PathBuf::from("./peek")))
        );
    }

    #[test]
    fn completions_takes_a_shell() {
        for (name, shell) in [
            ("bash", Shell::Bash),
            ("zsh", Shell::Zsh),
            ("fish", Shell::Fish),
        ] {
            assert_eq!(
                parse(&["completions", name], true),
                Ok(Action::Completions(shell))
            );
        }
        assert_eq!(
            parse(&["./completions"], true),
            Ok(Action::CopyFile(PathBuf::from("./completions")))
        );
    }

    #[test]
    fn completions_needs_one_known_shell() {
        assert_eq!(
            parse(&["completions"], true),
            Err("completions needs a shell: bash, zsh or fish".into())
        );
        assert_eq!(
            parse(&["completions", "tcsh"], true),
            Err("unknown shell 'tcsh'; expected bash, zsh or fish".into())
        );
        assert_eq!(
            parse(&["completions", "zsh", "bash"], true),
            Err("unexpected argument 'bash'".into())
        );
    }

    #[test]
    fn watch_takes_one_option() {
        assert_eq!(parse(&["watch"], true), Ok(Action::Watch));
        assert_eq!(
            parse(&["watch", "--install"], true),
            Ok(Action::WatchInstall)
        );
        assert_eq!(
            parse(&["watch", "--uninstall"], false),
            Ok(Action::WatchUninstall)
        );
        assert_eq!(
            parse(&["watch", "--bogus"], true),
            Err("unknown option '--bogus'".into())
        );
        assert_eq!(
            parse(&["watch", "now"], true),
            Err("unexpected argument 'now'".into())
        );
        assert_eq!(
            parse(&["watch", "--install", "x"], true),
            Err("unexpected argument 'x'".into())
        );
        assert_eq!(
            parse(&["./watch"], true),
            Ok(Action::CopyFile(PathBuf::from("./watch")))
        );
    }

    #[test]
    fn only_watch_takes_an_option() {
        assert_eq!(
            parse(&["peek", "--install"], true),
            Err("unexpected argument '--install'".into())
        );
    }

    #[test]
    fn unknown_option_is_rejected() {
        assert_eq!(
            parse(&["--bogus"], true),
            Err("unknown option '--bogus'".into())
        );
    }

    #[test]
    fn extra_arguments_are_rejected() {
        assert_eq!(
            parse(&["a", "b"], true),
            Err("unexpected argument 'b'".into())
        );
    }

    #[test]
    fn strips_exactly_one_trailing_newline() {
        assert_eq!(strip_trailing_newline("hello\n"), "hello");
        assert_eq!(strip_trailing_newline("hello\r\n"), "hello");
        assert_eq!(strip_trailing_newline("hello\n\n"), "hello\n");
        assert_eq!(strip_trailing_newline("hello"), "hello");
        assert_eq!(strip_trailing_newline(""), "");
    }

    #[test]
    fn keeps_leading_and_inner_whitespace() {
        assert_eq!(
            strip_trailing_newline("    indented\n  code \n"),
            "    indented\n  code "
        );
    }
}
