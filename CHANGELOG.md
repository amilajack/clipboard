# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.5](https://github.com/amilajack/clipboard/compare/v0.0.4...v0.0.5) - 2026-09-10

### Added

- add shell completions for bash, zsh and fish

## [0.0.4](https://github.com/amilajack/clipboard/compare/v0.0.3...v0.0.4) - 2026-09-10

### Added

- add cb peek to search clipboard history

### Other

- Merge pull request #20 from amilajack/feat/peek

## [0.0.2](https://github.com/amilajack/clipboard/compare/v0.0.1...v0.0.2) - 2026-09-10

### Fixed

- address codebase review findings

### Other

- bold
- transpose
- bump deps
- update man page
- add piping example to readme
- simplify examples
- add comparison char

### Details

- Copied text now survives `cb` exiting on Linux. X11 and Wayland clipboards
  only live as long as the program that set them, so `cb` leaves a background
  process serving the text until something else is copied.
- Native Wayland support, alongside X11.
- Piped input and files keep their leading whitespace. Only one trailing
  newline is dropped, and printing adds it back, so copying and printing a
  file reproduces it exactly.
- `cb FILE` copies the file even when stdin is not a terminal, such as in
  scripts and editor tasks, instead of copying empty input.
- Add `--help` and `--version`.
- Errors are reported as messages with exit status 1, or 2 for invalid
  arguments, instead of panics.
- Replace the unmaintained `atty` (RUSTSEC-2021-0145, RUSTSEC-2024-0375) with
  `std::io::IsTerminal`, and `copypasta` with `arboard`.
- Building no longer needs the X11 development libraries.
- Minimum supported Rust version is now 1.86.

## [0.0.1] - 2022-02-21

- Initial release.
