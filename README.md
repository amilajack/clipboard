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

# Show all options
cb --help
```

## History

`cb` remembers what it copies, and what it finds on the clipboard when it
prints it, so copies made in other programs show up too. `cb peek` opens that
history: newest first on the left, and on the right a preview of the selected
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
