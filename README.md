# clipboard

[![CI](https://github.com/amilajack/clipboard/actions/workflows/ci.yml/badge.svg)](https://github.com/amilajack/clipboard/actions/workflows/ci.yml)

A better command line clipboard

## Installation

```bash
cargo install clipboard-cli
```

## Usage

```bash
# Copy to os clipbard
cb package.json
# Pipe to os clipboard
cat package.json | cb
echo 'Hello World!' | cb

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
