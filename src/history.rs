//! Clipboard history: one JSON object per line, oldest first.

use std::env;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Where to keep history instead of the platform's data directory. An empty
/// value turns history off.
pub const HISTORY_ENV: &str = "CB_HISTORY_FILE";

/// Only the newest this many entries are kept.
const MAX_ENTRIES: usize = 500;

/// Larger copies aren't kept, so that reading history stays fast.
const MAX_ENTRY_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub text: String,
    /// The file the text was copied from, used to pick a syntax for its preview.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<PathBuf>,
    /// When it was copied, in seconds since the Unix epoch.
    pub time: u64,
}

/// The history file, or `None` if history is turned off.
pub fn path() -> Option<PathBuf> {
    resolve_path(env::var_os(HISTORY_ENV))
}

/// The history file, or an error saying history is turned off.
pub fn require_path() -> Result<PathBuf, String> {
    path().ok_or_else(|| {
        format!(
            "clipboard history is turned off because {} is empty",
            HISTORY_ENV
        )
    })
}

fn resolve_path(overridden: Option<OsString>) -> Option<PathBuf> {
    match overridden {
        Some(path) if path.is_empty() => None,
        Some(path) => Some(PathBuf::from(path)),
        None => dirs::data_dir().map(|dir| dir.join("cb").join("history.jsonl")),
    }
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// Adds `text` to history as the newest entry, unless history is turned off.
pub fn record(text: &str, source: Option<&Path>) -> Result<(), String> {
    let Some(path) = path() else {
        return Ok(());
    };
    let entry = Entry {
        text: text.to_owned(),
        source: source.map(|source| fs::canonicalize(source).unwrap_or_else(|_| source.to_owned())),
        time: now(),
    };
    let context = |e: io::Error| format!("{}: {}", path.display(), e);
    let mut entries = load(&path).map_err(context)?;
    if push(&mut entries, entry) {
        save(&path, &entries).map_err(context)
    } else {
        Ok(())
    }
}

/// Reads history, oldest first. A missing file is an empty history, and lines
/// that don't parse, say from a write that was cut short, are skipped.
pub fn load(path: &Path) -> io::Result<Vec<Entry>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut entries = Vec::new();
    for line in BufReader::new(file).lines() {
        if let Ok(entry) = serde_json::from_str(&line?) {
            entries.push(entry);
        }
    }
    Ok(entries)
}

/// Adds `entry` as the newest, replacing any earlier copy of the same text and
/// dropping the oldest entries past the limit. Returns whether anything changed:
/// empty and oversized text isn't kept, and text that is already the newest
/// entry is left alone.
fn push(entries: &mut Vec<Entry>, mut entry: Entry) -> bool {
    if entry.text.is_empty() || entry.text.len() > MAX_ENTRY_BYTES {
        return false;
    }
    if entries.last().is_some_and(|last| {
        last.text == entry.text && (entry.source.is_none() || entry.source == last.source)
    }) {
        return false;
    }
    if let Some(index) = entries.iter().rposition(|old| old.text == entry.text) {
        // Printing the clipboard records it without a source; keep the file
        // it originally came from so the preview still knows its syntax.
        let old = entries.remove(index);
        entry.source = entry.source.or(old.source);
    }
    entries.push(entry);
    if entries.len() > MAX_ENTRIES {
        entries.drain(..entries.len() - MAX_ENTRIES);
    }
    true
}

/// Replaces the history file in one step, so a reader never sees half of it.
fn save(path: &Path, entries: &[Entry]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    let saved = write_entries(&temp, entries).and_then(|()| fs::rename(&temp, path));
    if saved.is_err() {
        fs::remove_file(&temp).ok();
    }
    saved
}

fn write_entries(path: &Path, entries: &[Entry]) -> io::Result<()> {
    let mut out = BufWriter::new(create_private(path)?);
    for entry in entries {
        serde_json::to_writer(&mut out, entry)?;
        out.write_all(b"\n")?;
    }
    out.flush()
}

/// History can hold passwords and tokens, so only its owner may read it.
fn create_private(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(text: &str, time: u64) -> Entry {
        Entry {
            text: text.to_owned(),
            source: None,
            time,
        }
    }

    fn texts(entries: &[Entry]) -> Vec<&str> {
        entries.iter().map(|entry| entry.text.as_str()).collect()
    }

    /// A path in the temp directory that no other test uses.
    fn temp_path(name: &str) -> PathBuf {
        env::temp_dir().join(format!("cb-test-{}-{}", std::process::id(), name))
    }

    #[test]
    fn push_adds_the_newest_entry_last() {
        let mut entries = vec![entry("a", 1)];
        assert!(push(&mut entries, entry("b", 2)));
        assert_eq!(texts(&entries), ["a", "b"]);
    }

    #[test]
    fn push_moves_a_repeated_copy_to_the_end() {
        let mut entries = vec![entry("a", 1), entry("b", 2)];
        assert!(push(&mut entries, entry("a", 3)));
        assert_eq!(texts(&entries), ["b", "a"]);
        assert_eq!(entries[1].time, 3);
    }

    #[test]
    fn push_keeps_the_source_of_an_earlier_copy() {
        let mut entries = vec![
            Entry {
                source: Some("main.rs".into()),
                ..entry("fn main() {}", 1)
            },
            entry("b", 2),
        ];
        assert!(push(&mut entries, entry("fn main() {}", 3)));
        assert_eq!(entries[1].source, Some("main.rs".into()));
    }

    #[test]
    fn push_ignores_empty_oversized_and_unchanged_text() {
        let mut entries = vec![entry("a", 1)];
        assert!(!push(&mut entries, entry("", 2)));
        assert!(!push(
            &mut entries,
            entry(&"x".repeat(MAX_ENTRY_BYTES + 1), 2)
        ));
        assert!(!push(&mut entries, entry("a", 2)));
        assert_eq!(entries, [entry("a", 1)]);
    }

    #[test]
    fn push_drops_the_oldest_entries_past_the_limit() {
        let mut entries = (0..MAX_ENTRIES as u64)
            .map(|i| entry(&i.to_string(), i))
            .collect();
        assert!(push(&mut entries, entry("new", 1000)));
        assert_eq!(entries.len(), MAX_ENTRIES);
        assert_eq!(entries[0].text, "1");
        assert_eq!(entries[MAX_ENTRIES - 1].text, "new");
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = temp_path("round-trip");
        let path = dir.join("nested").join("history.jsonl");
        let entries = vec![
            Entry {
                source: Some("/tmp/a.rs".into()),
                ..entry("line one\n\t\"quoted\"\nline three", 1)
            },
            entry("second", 2),
        ];
        save(&path, &entries).unwrap();
        assert_eq!(load(&path).unwrap(), entries);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn load_skips_lines_it_cannot_read() {
        let path = temp_path("corrupt.jsonl");
        fs::write(
            &path,
            "{\"text\":\"good\",\"time\":1}\n{\"text\":\"cut sh\n\n",
        )
        .unwrap();
        assert_eq!(load(&path).unwrap(), [entry("good", 1)]);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_missing_file_is_an_empty_history() {
        assert_eq!(load(&temp_path("missing.jsonl")).unwrap(), []);
    }

    #[cfg(unix)]
    #[test]
    fn only_the_owner_can_read_history() {
        use std::os::unix::fs::PermissionsExt;

        let path = temp_path("private.jsonl");
        save(&path, &[entry("secret", 1)]).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn an_empty_override_turns_history_off() {
        assert_eq!(resolve_path(Some("".into())), None);
        assert_eq!(
            resolve_path(Some("h.jsonl".into())),
            Some(PathBuf::from("h.jsonl"))
        );
    }
}
