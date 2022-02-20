# clipboard

A better command line clipboard

## Installation

```bash
brew install cb
```

## Usage

```bash
# Copy to os clipbard
cb package.json
# Pipe to os clipboard
cat package.json | cb
echo 'Hello World!' | cb

# Read clipboard contents
cb | vim
```

## History

```bash
# queue peek
cb peek
cb peek 1
```

## Advanced

```bash
cb list

# aliasing
alias c='cb'
```
