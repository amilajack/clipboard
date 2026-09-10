use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use arboard::Clipboard;

const USAGE: &str = "\
Usage: cb [FILE]

Copy FILE, or whatever is piped in, to the system clipboard.
With no FILE and nothing piped in, print the clipboard.

Examples:
  cb package.json    Copy a file
  git diff | cb      Copy piped input
  cb | grep hello    Search the clipboard

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
}

/// Decides what to do from the arguments (excluding the program name).
///
/// An explicit file always wins over stdin, so `cb notes.txt` behaves the same
/// in a script or IDE task, where stdin is not a terminal, as it does at a prompt.
fn parse_args<I>(args: I, stdin_is_terminal: bool) -> Result<Action, String>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args = args.into_iter();
    let first = args.next();
    if let Some(extra) = args.next() {
        return Err(format!("unexpected argument '{}'", extra.to_string_lossy()));
    }

    let arg = match first {
        Some(arg) => arg,
        None if stdin_is_terminal => return Ok(Action::Print),
        None => return Ok(Action::CopyStdin),
    };
    match arg.to_str() {
        Some("-h" | "--help") => Ok(Action::Help),
        Some("-V" | "--version") => Ok(Action::Version),
        Some(flag) if flag.len() > 1 && flag.starts_with('-') => {
            Err(format!("unknown option '{}'", flag))
        }
        _ => Ok(Action::CopyFile(PathBuf::from(arg))),
    }
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
            copy(strip_trailing_newline(&text))
        }
        Action::CopyStdin => {
            let mut text = String::new();
            io::stdin()
                .read_to_string(&mut text)
                .map_err(|e| format!("stdin: {}", e))?;
            copy(strip_trailing_newline(&text))
        }
    }
}

fn print() -> Result<(), String> {
    let text = Clipboard::new()
        .and_then(|mut clipboard| clipboard.get_text())
        .map_err(|e| e.to_string())?;
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
