# clipboard

[![CI](https://github.com/amilajack/clipboard/actions/workflows/ci.yml/badge.svg)](https://github.com/amilajack/clipboard/actions/workflows/ci.yml)

A better command line clipboard

## Installation

```bash
cargo install clipboard-cli
```

## Usage

```bash
# Copy file to clipbard
cb package.json

# Pipe to clipboard
echo 'Hello World!' | cb
git diff | cb

# Read clipboard contents
cb | vim -

# Search clipboard contents
cb | grep hello
```

## Upcoming

```bash
# List clipboard history
cb list

# View previous clipboard
cb peek 1
cb p 1
```

## Comparison


|                | Single Command | Cross Platform    | Simple API | History Peek |
| -------------- | -------------- | ----------------- | ---------- | ------------ |
| clipboard      | ✅             | ✅                | ✅         | ✅           |
| pbcopy/pbpaste | ❌             | ❌ (macOS only)   | ✅         | ❌           |
| xclip          | ✅             | ❌ (linux only)   | ❌         | ❌           |
| clip           | ✅             | ❌ (windows only) | ✅         | ❌           |
