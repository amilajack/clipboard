# clipboard

[![CI](https://github.com/amilajack/clipboard/actions/workflows/ci.yml/badge.svg)](https://github.com/amilajack/clipboard/actions/workflows/ci.yml)

A better command line clipboard

## Installation

```bash
cargo install clipboard-cli
```

## Usage

```bash
# Copy file to clipboard
cb package.json

# Pipe to clipboard
echo 'Hello World!' | cb
git diff | cb

# Read clipboard contents
cb | vim -

# Search clipboard contents
cb | grep hello

# Search clipboard history, preview it, and copy an entry again
cb peek

# Record everything you copy, in any program, from now on and at every login
cb watch --install

# Show all options
cb --help
```

## History

`cb` remembers what it copies, and what it finds on the clipboard when it
prints it. To record everything you copy, in any program, run
[`cb watch`](#recording-everything-you-copy). `cb peek` opens that history: newest first on the left, and on the right a preview of the selected
entry, syntax highlighted with [bat](https://github.com/sharkdp/bat)'s syntaxes
and themes.

| Key                         | Action                                     |
| --------------------------- | ------------------------------------------ |
| Type                        | Search every line of every entry           |
| `↑` `↓`                     | Move through the entries                   |
| `Ctrl-R` `Ctrl-S`           | Step to the next older or newer match      |
| `PgUp` `PgDn`, `Shift-↑` `Shift-↓` | Scroll the preview                  |
| `Backspace` `Ctrl-W` `Ctrl-U` | Delete a character, a word, or the search |
| `Enter`                     | Copy the selected entry and exit           |
| `Esc`, `Ctrl-C`             | Exit without copying                       |

The search is case-sensitive only if it contains an uppercase letter.

History holds the last 500 copies, up to 1 MiB each, in a file only you can
read: `~/.local/share/cb/history.jsonl` on Linux,
`~/Library/Application Support/cb/history.jsonl` on macOS, and
`%APPDATA%\cb\history.jsonl` on Windows. Set `CB_HISTORY_FILE` to keep it
somewhere else, or to an empty string to turn history off. The preview theme
comes from `CB_THEME`, or else `BAT_THEME`.

### Recording everything you copy

`cb` on its own only sees the clipboard when you run it. `cb watch` checks it
every 2 seconds and records each new text copy, whichever program made it:

```bash
cb watch --install     # start now, and whenever you log in
cb watch --uninstall   # stop, now and at login
cb watch               # or run it in the foreground; Ctrl-C stops it
```

`--install` adds a desktop autostart entry on Linux
(`~/.config/autostart/cb-watch.desktop`), a LaunchAgent on macOS
(`~/Library/LaunchAgents/com.github.amilajack.cb.watch.plist`), and an entry
under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` on Windows. The
entry runs the `cb` you installed it with, so install again if you move `cb`.
Window managers without desktop autostart, like sway and i3, need `cb watch`
started from their config instead. Only one watcher runs at a time.

Copies that password managers mark as secret are never recorded, by `cb watch`
or anything else in `cb`: the `x-kde-passwordManagerHint` type on Linux, the
[nspasteboard.org](http://nspasteboard.org) types on macOS, and
`ExcludeClipboardContentFromMonitorProcessing` and its relatives on Windows.
Only text is recorded, and a copy replaced within 2 seconds may be missed.

## Completions

`cb` can tab-complete its commands, options and file names in bash, zsh and
fish. Turn it on with one line in your shell's config:

```bash
# ~/.bashrc
eval "$(cb completions bash)"

# ~/.zshrc, after compinit
source <(cb completions zsh)
```

In fish, save the script where fish looks for completions:

```fish
cb completions fish > ~/.config/fish/completions/cb.fish
```

The Debian package installs all three for you.

## Upcoming

```bash
# Print history without the picker
cb list

# Print a previous clipboard entry
cb peek 1
```

## Releasing

Releases are automated with [release-plz](https://release-plz.dev). Write commit
messages as [Conventional Commits](https://www.conventionalcommits.org): `fix:`,
`feat:`, and `feat!:` or a `BREAKING CHANGE:` footer for breaking changes.

Every merge to `main` opens or updates a release PR that bumps the version by
semver and updates `CHANGELOG.md`. Merging that PR publishes the crate to
crates.io, tags the release, and attaches the binaries built by CI.
`ci`, `docs`, `chore`, `build`, `style` and `test` commits are left out of the
changelog.

## Comparison

|                | **clipboard** | pbcopy/pbpaste  | xclip           | clip              |
| -------------- | ------------- | --------------- | --------------- | ----------------- |
| Single Command | ✅            | ❌              | ✅              | ✅                |
| Cross Platform | ✅            | ❌ (macOS only) | ❌ (linux only) | ❌ (windows only) |
| Simple API     | ✅            | ✅              | ❌              | ✅                |
| History Peek   | ✅            | ❌              | ❌              | ❌                |
