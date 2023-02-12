# clipboard

[![CI](https://github.com/amilajack/clipboard/actions/workflows/ci.yml/badge.svg)](https://github.com/amilajack/clipboard/actions/workflows/ci.yml)

A better command line clipboard

## Installation

### macOS

```bash
brew install diskus
```

### Debian

```bash
wget "https://github.com/amilajack/clipboard/releases/download/v0.0.1/clipboard_0.0.1_amd64.deb"
sudo dpkg -i clipboard_0.0.1_amd64.deb
```

### Other

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

|                | **clipboard** | pbcopy/pbpaste  | xclip           | clip              |
| -------------- | ------------- | --------------- | --------------- | ----------------- |
| Single Command | ✅            | ❌              | ✅              | ✅                |
| Cross Platform | ✅            | ❌ (macOS only) | ❌ (linux only) | ❌ (windows only) |
| Simple API     | ✅            | ✅              | ❌              | ✅                |
| History Peak   | ✅            | ❌              | ❌              | ❌                |
