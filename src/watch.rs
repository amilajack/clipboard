//! `cb watch`: record everything copied, not only what goes through cb.

use std::env;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{self, Command, Stdio};
use std::thread;
use std::time::Duration;

use arboard::Clipboard;

use crate::history::{self, Use};
use crate::{platform, remember};

/// How often to look at the clipboard where the platform doesn't say when it
/// changes.
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Watches the clipboard until stopped, adding each new copy to history.
pub fn run() -> Result<(), String> {
    let address = control::address(&history::require_path()?);
    // The address is taken before anything slow, so that of two watchers
    // starting at once, the second finds the first.
    let stop_address = address.clone();
    let listening = control::listen(&address, move || {
        control::release(&stop_address);
        process::exit(0);
    })
    .map_err(|e| format!("{}: {}", address.display(), e))?;
    if !listening {
        eprintln!("cb: already watching the clipboard");
        return Ok(());
    }
    let mut clipboard = Clipboard::new().map_err(|e| {
        control::release(&address);
        e.to_string()
    })?;
    let mut changes = platform::Changes::listen();
    match &changes {
        Some(changes) => eprintln!(
            "cb: watching the clipboard through {}; press Ctrl-C to stop",
            changes.name()
        ),
        None => eprintln!(
            "cb: watching the clipboard, checking every {} seconds; press Ctrl-C to stop",
            POLL_INTERVAL.as_secs()
        ),
    }
    hide_console();

    let mut watcher = Watcher::default();
    let mut count = platform::change_count();
    loop {
        check(&mut clipboard, &mut watcher);
        match changes.as_mut() {
            Some(listener) => {
                if !listener.wait() {
                    eprintln!(
                        "cb: clipboard notifications stopped; checking every {} seconds instead",
                        POLL_INTERVAL.as_secs()
                    );
                    changes = None;
                }
            }
            None => poll(&mut count),
        }
    }
}

/// Reads the clipboard, and records it if it holds a new copy.
fn check(clipboard: &mut Clipboard, watcher: &mut Watcher) {
    // Right after a copy, the clipboard can be busy: on Windows, other
    // programs open it to read it too. With nothing prompting a later look,
    // give it a few tries before letting this change go.
    for attempt in 0..5 {
        match clipboard.get_text() {
            Ok(text) => {
                if watcher.observe(Some(&text), platform::is_concealed) {
                    remember(&text, None, Use::Seen);
                }
                return;
            }
            // Empty, or holding something other than text.
            Err(arboard::Error::ContentNotAvailable) => {
                watcher.observe(None, || false);
                return;
            }
            // The connection to the display may have broken too, so the next
            // try gets a fresh one.
            Err(_) => {
                thread::sleep(Duration::from_millis(100 << attempt));
                if let Ok(fresh) = Clipboard::new() {
                    *clipboard = fresh;
                }
            }
        }
    }
}

/// Sleeps until the clipboard may have changed, looking every
/// `POLL_INTERVAL`. Where the platform counts changes, as macOS and Windows
/// do, the count is what's looked at, and the clipboard is only read when it
/// moves.
fn poll(count: &mut Option<u64>) {
    loop {
        thread::sleep(POLL_INTERVAL);
        let now = platform::change_count();
        if now.is_none() || now != *count {
            *count = now;
            return;
        }
    }
}

/// Makes `cb watch` start at login, and starts it now.
pub fn install() -> Result<(), String> {
    let address = control::address(&history::require_path()?);
    let exe = env::current_exe().map_err(|e| e.to_string())?;
    println!("{}", autostart::install(&exe)?);
    if control::is_running(&address) {
        println!("Already watching the clipboard");
        return Ok(());
    }
    start_in_background(&exe).map_err(|e| format!("failed to start cb watch: {}", e))?;
    // Wait for it to answer, so that a watcher that can't start, say for
    // want of a display, is reported here rather than failing silently.
    for _ in 0..30 {
        thread::sleep(Duration::from_millis(100));
        if control::is_running(&address) {
            println!("Watching the clipboard");
            return Ok(());
        }
    }
    Err("cb watch didn't start; run it in the foreground to see why".to_owned())
}

/// Stops `cb watch`, now and at login.
pub fn uninstall() -> Result<(), String> {
    match autostart::uninstall()? {
        Some(removed) => println!("{}", removed),
        None => println!("cb watch wasn't set to start at login"),
    }
    if control::stop(&control::address(&history::require_path()?)) {
        println!("Stopped watching the clipboard");
    }
    Ok(())
}

fn start_in_background(exe: &Path) -> io::Result<()> {
    let mut command = Command::new(exe);
    command
        .arg("watch")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Its own process group keeps a Ctrl-C at this prompt from reaching it.
        command.current_dir("/").process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }
    command.spawn().map(drop)
}

/// Started at login, `cb` gets a console window all to itself on Windows.
/// Close it, unless a shell shares it.
#[cfg(windows)]
fn hide_console() {
    extern "system" {
        fn GetConsoleProcessList(list: *mut u32, count: u32) -> u32;
        fn FreeConsole() -> i32;
    }
    let mut processes = [0u32; 2];
    // SAFETY: the list is as long as the count says, and FreeConsole takes
    // nothing.
    unsafe {
        if GetConsoleProcessList(processes.as_mut_ptr(), 2) == 1 {
            FreeConsole();
        }
    }
}

#[cfg(not(windows))]
fn hide_console() {}

/// Decides which clipboard reads are new copies.
#[derive(Debug, Default)]
struct Watcher {
    /// A hash of the text last seen, recorded or not. Keeping only a hash
    /// keeps skipped secrets out of memory.
    last: Option<u64>,
}

impl Watcher {
    /// Takes what the clipboard holds now, `None` for nothing or something
    /// other than text, and returns whether to record it: whether it is a new
    /// copy that nobody asked to keep out of history.
    fn observe(&mut self, text: Option<&str>, concealed: impl FnOnce() -> bool) -> bool {
        let hash = text.map(|text| fnv1a(text.as_bytes()));
        if hash == self.last {
            return false;
        }
        self.last = hash;
        text.is_some() && !concealed()
    }
}

/// FNV-1a, a hash that stays the same across Rust versions, unlike std's.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// How other cb processes find a running watcher and ask it to stop, over a
/// Unix socket, or on Windows, where std has none, a localhost port kept in a
/// file.
mod control {
    use super::*;

    #[cfg(windows)]
    use std::net::{Ipv4Addr, TcpListener as Listener, TcpStream as Stream};
    #[cfg(unix)]
    use std::os::unix::net::{UnixListener as Listener, UnixStream as Stream};
    use std::path::PathBuf;

    /// What a watcher says first to anything that connects, so that a stale
    /// address can't be mistaken for one.
    const GREETING: &str = "cb-watch";

    const TIMEOUT: Duration = Duration::from_secs(1);

    /// Where the watcher for `history` listens. There is one watcher per
    /// history file, and socket paths are short, so the name is a hash.
    pub fn address(history: &Path) -> PathBuf {
        let dir = dirs::runtime_dir().unwrap_or_else(env::temp_dir);
        let hash = fnv1a(history.as_os_str().as_encoded_bytes());
        dir.join(format!("cb-watch-{:016x}", hash))
    }

    /// Makes this process the watcher at `address` and serves it in the
    /// background, calling `on_stop` when asked to. Returns false, and serves
    /// nothing, if another watcher already has the address.
    pub fn listen(address: &Path, on_stop: impl Fn() + Send + 'static) -> io::Result<bool> {
        let Some(listener) = claim(address)? else {
            return Ok(false);
        };
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                if wants_stop(stream) {
                    on_stop();
                }
            }
        });
        Ok(true)
    }

    /// Gives up `address`, for a watcher on its way out.
    pub fn release(address: &Path) {
        fs::remove_file(address).ok();
    }

    /// Takes `address`, or returns `None` if a watcher answers there. Taking
    /// it can only succeed for one process at a time, so of two watchers
    /// starting at once, one gets it and the other finds it taken.
    fn claim(address: &Path) -> io::Result<Option<Listener>> {
        for _ in 0..3 {
            match bind(address) {
                Ok(listener) => return Ok(Some(listener)),
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::AddrInUse | io::ErrorKind::AlreadyExists
                    ) =>
                {
                    if is_running(address) {
                        return Ok(None);
                    }
                    // Left behind by a watcher that was killed.
                    match fs::remove_file(address) {
                        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                        _ => {}
                    }
                }
                Err(e) => return Err(e),
            }
        }
        Err(io::ErrorKind::AddrInUse.into())
    }

    pub fn is_running(address: &Path) -> bool {
        reach(address).is_some()
    }

    /// Asks the watcher at `address` to exit, and waits until it has.
    /// Returns whether there was one.
    pub fn stop(address: &Path) -> bool {
        let Some(mut stream) = reach(address) else {
            return false;
        };
        if stream.write_all(b"stop\n").is_err() {
            return false;
        }
        // The connection closes when the watcher exits.
        stream.read_to_end(&mut Vec::new()).ok();
        true
    }

    fn wants_stop(mut stream: Stream) -> bool {
        stream.set_read_timeout(Some(TIMEOUT)).ok();
        if writeln!(stream, "{}", GREETING).is_err() {
            return false;
        }
        let mut request = String::new();
        BufReader::new(&stream).read_line(&mut request).is_ok() && request.trim_end() == "stop"
    }

    fn reach(address: &Path) -> Option<Stream> {
        let stream = connect(address).ok()?;
        stream.set_read_timeout(Some(TIMEOUT)).ok()?;
        let mut greeting = String::new();
        BufReader::new(&stream).read_line(&mut greeting).ok()?;
        (greeting.trim_end() == GREETING).then_some(stream)
    }

    /// Fails if the socket file exists.
    #[cfg(unix)]
    fn bind(address: &Path) -> io::Result<Listener> {
        Listener::bind(address)
    }

    #[cfg(unix)]
    fn connect(address: &Path) -> io::Result<Stream> {
        Stream::connect(address)
    }

    /// Fails if the port file exists. It's written to the side and then
    /// linked into place, so it appears whole or not at all.
    #[cfg(windows)]
    fn bind(address: &Path) -> io::Result<Listener> {
        use std::sync::atomic::{AtomicUsize, Ordering};

        // Unique to this attempt, so attempts never share the file.
        static ATTEMPTS: AtomicUsize = AtomicUsize::new(0);
        let attempt = ATTEMPTS.fetch_add(1, Ordering::Relaxed);
        let listener = Listener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let temp = address.with_extension(format!("{}-{}.tmp", process::id(), attempt));
        fs::write(&temp, listener.local_addr()?.port().to_string())?;
        let linked = fs::hard_link(&temp, address);
        fs::remove_file(&temp).ok();
        linked.map(|()| listener)
    }

    #[cfg(windows)]
    fn connect(address: &Path) -> io::Result<Stream> {
        let port: u16 = fs::read_to_string(address)?
            .trim()
            .parse()
            .map_err(io::Error::other)?;
        Stream::connect_timeout(&(Ipv4Addr::LOCALHOST, port).into(), TIMEOUT)
    }
}

/// Starting `cb watch` at login, the way each platform does it.
mod autostart {
    #[allow(unused_imports)]
    use super::*;

    #[cfg(target_os = "macos")]
    const LAUNCH_AGENT: &str = "com.github.amilajack.cb.watch";

    /// An XDG autostart entry, which GNOME, KDE, Xfce and most other desktops
    /// run at login.
    #[cfg_attr(
        not(all(
            unix,
            not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
        )),
        allow(dead_code)
    )]
    pub fn desktop_entry(exe: &Path) -> String {
        format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=cb clipboard history\n\
             Comment=Records what you copy, for cb peek\n\
             Exec={} watch\n\
             Terminal=false\n\
             NoDisplay=true\n\
             X-GNOME-Autostart-enabled=true\n",
            exec_quote(&exe.to_string_lossy())
        )
    }

    /// Quotes an argument for a desktop entry's `Exec` key. Quoting escapes a
    /// few characters with backslashes, and then, as in any desktop entry
    /// value, backslashes are escaped again.
    #[cfg_attr(
        not(all(
            unix,
            not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
        )),
        allow(dead_code)
    )]
    fn exec_quote(arg: &str) -> String {
        let mut quoted = String::from("\"");
        for c in arg.chars() {
            match c {
                '"' | '`' | '$' | '\\' => {
                    quoted.push('\\');
                    quoted.push(c);
                }
                '%' => quoted.push_str("%%"),
                c => quoted.push(c),
            }
        }
        quoted.push('"');
        quoted.replace('\\', "\\\\")
    }

    /// A launchd agent that starts the watcher at login, and again if it
    /// fails, but not after it's been stopped.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn launch_agent(label: &str, exe: &Path) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
        <string>watch</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
    </dict>
    <key>LimitLoadToSessionType</key>
    <string>Aqua</string>
    <key>ProcessType</key>
    <string>Background</string>
</dict>
</plist>
"#,
            xml_escape(label),
            xml_escape(&exe.to_string_lossy())
        )
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    fn xml_escape(text: &str) -> String {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }

    #[cfg(all(
        unix,
        not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
    ))]
    fn entry_path() -> Result<std::path::PathBuf, String> {
        dirs::config_dir()
            .map(|dir| dir.join("autostart").join("cb-watch.desktop"))
            .ok_or_else(|| "couldn't find your config directory".to_owned())
    }

    #[cfg(target_os = "macos")]
    fn entry_path() -> Result<std::path::PathBuf, String> {
        dirs::home_dir()
            .map(|home| {
                home.join("Library")
                    .join("LaunchAgents")
                    .join(format!("{}.plist", LAUNCH_AGENT))
            })
            .ok_or_else(|| "couldn't find your home directory".to_owned())
    }

    #[cfg(all(
        unix,
        not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
    ))]
    fn entry(exe: &Path) -> String {
        desktop_entry(exe)
    }

    #[cfg(target_os = "macos")]
    fn entry(exe: &Path) -> String {
        launch_agent(LAUNCH_AGENT, exe)
    }

    /// Returns what it did, for the user.
    #[cfg(any(
        target_os = "macos",
        all(
            unix,
            not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
        )
    ))]
    pub fn install(exe: &Path) -> Result<String, String> {
        let path = entry_path()?;
        let context = |e: io::Error| format!("{}: {}", path.display(), e);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(context)?;
        }
        fs::write(&path, entry(exe)).map_err(context)?;
        Ok(format!("Added {}", path.display()))
    }

    /// Returns what it did, or `None` if there was nothing to remove.
    #[cfg(any(
        target_os = "macos",
        all(
            unix,
            not(any(target_os = "macos", target_os = "android", target_os = "emscripten"))
        )
    ))]
    pub fn uninstall() -> Result<Option<String>, String> {
        let path = entry_path()?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(Some(format!("Removed {}", path.display()))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("{}: {}", path.display(), e)),
        }
    }

    #[cfg(windows)]
    const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    #[cfg(windows)]
    const RUN_VALUE: &str = "cb watch";

    #[cfg(windows)]
    pub fn install(exe: &Path) -> Result<String, String> {
        use std::os::windows::process::CommandExt;

        // reg takes quotes inside the value escaped with backslashes;
        // `raw_arg` keeps std from quoting them all over again.
        let data = format!(r#"/d "\"{}\" watch""#, exe.display());
        let output = Command::new("reg")
            .args(["add", RUN_KEY, "/v", RUN_VALUE, "/t", "REG_SZ", "/f"])
            .raw_arg(data)
            .output()
            .map_err(|e| format!("reg: {}", e))?;
        if !output.status.success() {
            let message = String::from_utf8_lossy(&output.stderr);
            return Err(format!("reg: {}", message.trim()));
        }
        Ok(format!("Added \"{}\" to {}", RUN_VALUE, RUN_KEY))
    }

    #[cfg(windows)]
    pub fn uninstall() -> Result<Option<String>, String> {
        // reg fails when there's nothing to delete, which is fine here.
        let deleted = Command::new("reg")
            .args(["delete", RUN_KEY, "/v", RUN_VALUE, "/f"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| format!("reg: {}", e))?
            .success();
        Ok(deleted.then(|| format!("Removed \"{}\" from {}", RUN_VALUE, RUN_KEY)))
    }

    #[cfg(not(any(unix, windows)))]
    pub fn install(_exe: &Path) -> Result<String, String> {
        Err("starting at login isn't supported here".to_owned())
    }

    #[cfg(not(any(unix, windows)))]
    pub fn uninstall() -> Result<Option<String>, String> {
        Ok(None)
    }

    #[cfg(any(target_os = "android", target_os = "emscripten"))]
    pub fn install(_exe: &Path) -> Result<String, String> {
        Err("starting at login isn't supported here".to_owned())
    }

    #[cfg(any(target_os = "android", target_os = "emscripten"))]
    pub fn uninstall() -> Result<Option<String>, String> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::sync::mpsc;

    #[test]
    fn records_each_new_copy_once() {
        let mut watcher = Watcher::default();
        assert!(watcher.observe(Some("a"), || false));
        assert!(!watcher.observe(Some("a"), || false));
        assert!(watcher.observe(Some("b"), || false));
        assert!(watcher.observe(Some("a"), || false));
    }

    #[test]
    fn a_copy_counts_again_after_something_that_isnt_text() {
        let mut watcher = Watcher::default();
        assert!(watcher.observe(Some("a"), || false));
        assert!(!watcher.observe(None, || false));
        assert!(watcher.observe(Some("a"), || false));
    }

    #[test]
    fn concealed_copies_are_skipped_and_checked_once() {
        let mut watcher = Watcher::default();
        let checks = Cell::new(0);
        let concealed = || {
            checks.set(checks.get() + 1);
            true
        };
        assert!(!watcher.observe(Some("hunter2"), concealed));
        assert!(!watcher.observe(Some("hunter2"), concealed));
        assert_eq!(checks.get(), 1);
        assert!(watcher.observe(Some("not a secret"), || false));
    }

    #[test]
    fn fnv1a_matches_its_reference_values() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn a_running_watcher_can_be_found_and_stopped() {
        let address = env::temp_dir().join(format!("cb-test-{}-watch", process::id()));
        assert!(!control::is_running(&address));
        assert!(!control::stop(&address));

        let (stopped, stops) = mpsc::channel();
        assert!(control::listen(&address, move || stopped.send(()).unwrap()).unwrap());
        assert!(control::is_running(&address));
        assert!(stops.try_recv().is_err(), "checking must not stop it");
        assert!(control::stop(&address));
        stops.recv_timeout(Duration::from_secs(2)).unwrap();
        control::release(&address);
    }

    #[test]
    fn of_watchers_starting_together_only_one_listens() {
        let address = env::temp_dir().join(format!("cb-test-{}-race", process::id()));
        let starts: Vec<_> = (0..8)
            .map(|_| {
                let address = address.clone();
                thread::spawn(move || control::listen(&address, || {}).unwrap())
            })
            .collect();
        let listening = starts
            .into_iter()
            .map(|start| start.join().unwrap())
            .filter(|&listening| listening)
            .count();
        assert_eq!(listening, 1);
        control::release(&address);
    }

    #[test]
    fn an_address_left_behind_is_taken_over() {
        let address = env::temp_dir().join(format!("cb-test-{}-stale", process::id()));
        #[cfg(unix)]
        drop(std::os::unix::net::UnixListener::bind(&address).unwrap());
        #[cfg(windows)]
        fs::write(&address, "1").unwrap();
        assert!(address.exists());
        assert!(!control::is_running(&address));

        assert!(control::listen(&address, || {}).unwrap());
        assert!(control::is_running(&address));
        control::release(&address);
    }

    #[test]
    fn watchers_are_told_apart_by_history_file() {
        let a = control::address(Path::new("/a/history.jsonl"));
        let b = control::address(Path::new("/b/history.jsonl"));
        assert_ne!(a, b);
        assert_eq!(a, control::address(Path::new("/a/history.jsonl")));
    }

    #[test]
    fn desktop_entries_quote_the_path() {
        let entry = autostart::desktop_entry(Path::new("/home/a b/c$d\"e\\f%g"));
        assert!(
            entry.contains(r#"Exec="/home/a b/c\\$d\\"e\\\\f%%g" watch"#),
            "{}",
            entry
        );
        assert!(entry.starts_with("[Desktop Entry]\nType=Application\n"));
    }

    #[test]
    fn launch_agents_escape_the_path() {
        let agent = autostart::launch_agent("x", Path::new("/Users/a&b/<cb>"));
        assert!(
            agent.contains("<string>/Users/a&amp;b/&lt;cb&gt;</string>"),
            "{}",
            agent
        );
        assert!(agent.contains("<key>RunAtLoad</key>\n    <true/>"));
    }
}
