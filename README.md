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

# Show all options
cb --help
```

## Upcoming

```bash
# List clipboard history
cb list

# View previous clipboard
cb peek 1
cb p 1
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
| History Peek   | 🚧 planned    | ❌              | ❌              | ❌                |
