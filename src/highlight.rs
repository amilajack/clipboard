//! Syntax highlighting for `cb peek` previews, with bat's syntaxes and themes.

use std::env;
use std::iter;
use std::path::Path;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use serde::de::IgnoredAny;
use syntect::highlighting::{
    FontStyle, HighlightIterator, HighlightState, Highlighter as ThemeHighlighter, Theme,
};
use syntect::parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet};
use two_face::theme::{EmbeddedLazyThemeSet, EmbeddedThemeName};

/// bat's default theme.
const DEFAULT_THEME: EmbeddedThemeName = EmbeddedThemeName::MonokaiExtended;

/// Columns between tab stops, as in bat.
const TAB_WIDTH: usize = 4;

/// Longer lines are shown without highlighting. Parsing gets slow on things
/// like minified JSON, and the preview only has room for the start of them.
const MAX_HIGHLIGHTED_LINE_BYTES: usize = 4096;

/// Lines past this are shown without highlighting, so that scrolling deep into
/// a large entry doesn't stall on parsing everything above it.
const MAX_HIGHLIGHTED_LINES: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Colors {
    /// `NO_COLOR` is set.
    None,
    /// The 256-color palette, for terminals that don't advertise true color.
    Ansi256,
    TrueColor,
}

pub struct Highlighter {
    syntaxes: SyntaxSet,
    theme: Theme,
    colors: Colors,
}

impl Highlighter {
    /// Takes the theme from `CB_THEME`, falling back to `BAT_THEME` and then
    /// bat's default, and color support from `NO_COLOR` and `COLORTERM`.
    pub fn from_env() -> Result<Self, String> {
        let theme = match env::var("CB_THEME") {
            Ok(name) if !name.is_empty() => {
                find_theme(&name).ok_or_else(|| format!("unknown theme '{}' in CB_THEME", name))?
            }
            // BAT_THEME may name a theme of the user's own that we don't have.
            _ => env::var("BAT_THEME")
                .ok()
                .and_then(|name| find_theme(&name))
                .unwrap_or(DEFAULT_THEME),
        };
        let colors = if env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty()) {
            Colors::None
        } else if cfg!(windows)
            || matches!(env::var("COLORTERM").as_deref(), Ok("truecolor" | "24bit"))
        {
            // Windows consoles have handled true color since Windows 10.
            Colors::TrueColor
        } else {
            Colors::Ansi256
        };
        Ok(Self::new(theme, colors))
    }

    pub fn new(theme: EmbeddedThemeName, colors: Colors) -> Self {
        Self {
            syntaxes: two_face::syntax::extra_newlines(),
            theme: two_face::theme::extra().get(theme).clone(),
            colors,
        }
    }

    /// Picks a syntax from the file the text was copied from, or failing
    /// that, from the text itself.
    fn syntax_for(&self, text: &str, source: Option<&Path>) -> &SyntaxReference {
        source
            .and_then(|path| self.syntax_for_path(path))
            .or_else(|| self.syntax_for_text(text))
            .unwrap_or_else(|| self.syntaxes.find_syntax_plain_text())
    }

    fn syntax_for_path(&self, path: &Path) -> Option<&SyntaxReference> {
        // Whole names first, for files like `Dockerfile` and `Makefile`.
        let name = path.file_name()?.to_str()?;
        self.syntaxes.find_syntax_by_extension(name).or_else(|| {
            let extension = path.extension()?.to_str()?;
            self.syntaxes.find_syntax_by_extension(extension)
        })
    }

    fn syntax_for_text(&self, text: &str) -> Option<&SyntaxReference> {
        let text = text.trim_start();
        let first_line = text.lines().next()?;
        if let Some(syntax) = self.syntaxes.find_syntax_by_first_line(first_line) {
            return Some(syntax);
        }
        // Things often piped into cb that have no telltale first line.
        let name = if first_line.starts_with("diff ")
            || (first_line.starts_with("--- ") && text.contains("\n+++ "))
        {
            "Diff"
        } else if (text.starts_with('{') || text.starts_with('['))
            && serde_json::from_str::<IgnoredAny>(text).is_ok()
        {
            "JSON"
        } else {
            return None;
        };
        self.syntaxes.find_syntax_by_name(name)
    }

    fn style(&self, style: syntect::highlighting::Style) -> Style {
        let mut out = Style::new();
        if let Some(color) = convert_color(style.foreground, self.colors) {
            out = out.fg(color);
        }
        for (font, modifier) in [
            (FontStyle::BOLD, Modifier::BOLD),
            (FontStyle::ITALIC, Modifier::ITALIC),
            (FontStyle::UNDERLINE, Modifier::UNDERLINED),
        ] {
            if style.font_style.contains(font) {
                out = out.add_modifier(modifier);
            }
        }
        out
    }
}

fn find_theme(name: &str) -> Option<EmbeddedThemeName> {
    EmbeddedLazyThemeSet::theme_names()
        .iter()
        .copied()
        .find(|theme| theme.as_name().eq_ignore_ascii_case(name))
}

/// Converts a theme color the way bat does. Its `ansi` and `base16` themes
/// use alpha 0 to mean palette color number `r`, and alpha 1 to mean the
/// terminal's default color. The background is always left to the terminal.
fn convert_color(color: syntect::highlighting::Color, colors: Colors) -> Option<Color> {
    match (color.a, colors) {
        (_, Colors::None) | (1, _) => None,
        (0, _) => Some(match color.r {
            0 => Color::Black,
            1 => Color::Red,
            2 => Color::Green,
            3 => Color::Yellow,
            4 => Color::Blue,
            5 => Color::Magenta,
            6 => Color::Cyan,
            7 => Color::Gray,
            n => Color::Indexed(n),
        }),
        (_, Colors::TrueColor) => Some(Color::Rgb(color.r, color.g, color.b)),
        (_, Colors::Ansi256) => Some(Color::Indexed(ansi256(color.r, color.g, color.b))),
    }
}

/// The nearest color in xterm's 256-color palette: the gray ramp for grays,
/// the 6×6×6 color cube for everything else.
fn ansi256(r: u8, g: u8, b: u8) -> u8 {
    if r == g && g == b {
        return match r {
            0..8 => 16,
            249.. => 231,
            _ => 232 + ((r - 8) / 10).min(23),
        };
    }
    let level = |v: u8| match v {
        0..48 => 0,
        48..115 => 1,
        _ => (v - 35) / 40,
    };
    16 + 36 * level(r) + 6 * level(g) + level(b)
}

/// Appends `text` to `spans` as it should appear on screen: tabs expand to the
/// next tab stop, line endings are dropped, and other control characters
/// become `�`, so clipboard contents can't send escape sequences to the
/// terminal. `column` tracks the position within the line across calls.
pub fn push_span(spans: &mut Vec<Span<'static>>, column: &mut usize, text: &str, style: Style) {
    let mut shown = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\t' => {
                let width = TAB_WIDTH - *column % TAB_WIDTH;
                shown.extend(iter::repeat_n(' ', width));
                *column += width;
            }
            '\n' | '\r' => {}
            c => {
                shown.push(if c.is_control() {
                    char::REPLACEMENT_CHARACTER
                } else {
                    c
                });
                *column += 1;
            }
        }
    }
    if !shown.is_empty() {
        spans.push(Span::styled(shown, style));
    }
}

/// One entry's highlighted lines, produced as they're scrolled into view.
pub struct Preview {
    text: String,
    /// Byte offset of the first line not highlighted yet.
    next: usize,
    parse: ParseState,
    highlight: HighlightState,
    /// Set if the parser gives up, after which lines are shown plain.
    failed: bool,
    lines: Vec<Line<'static>>,
    pub syntax: String,
    pub line_count: usize,
}

impl Preview {
    pub fn new(highlighter: &Highlighter, text: &str, source: Option<&Path>) -> Self {
        let syntax = highlighter.syntax_for(text, source);
        let theme = ThemeHighlighter::new(&highlighter.theme);
        Self {
            text: text.to_owned(),
            next: 0,
            parse: ParseState::new(syntax),
            highlight: HighlightState::new(&theme, ScopeStack::new()),
            failed: false,
            lines: Vec::new(),
            syntax: syntax.name.clone(),
            line_count: text.lines().count().max(1),
        }
    }

    /// The first `count` lines, or all of them if there are fewer.
    pub fn lines(&mut self, highlighter: &Highlighter, count: usize) -> &[Line<'static>] {
        let theme = ThemeHighlighter::new(&highlighter.theme);
        while self.lines.len() < count && self.next < self.text.len() {
            let rest = &self.text[self.next..];
            let line = &rest[..rest.find('\n').map_or(rest.len(), |end| end + 1)];
            self.next += line.len();

            let plain = self.failed
                || highlighter.colors == Colors::None
                || line.len() > MAX_HIGHLIGHTED_LINE_BYTES
                || self.lines.len() >= MAX_HIGHLIGHTED_LINES;
            let ops = if plain {
                None
            } else {
                match self.parse.parse_line(line, &highlighter.syntaxes) {
                    Ok(ops) => Some(ops),
                    Err(_) => {
                        self.failed = true;
                        None
                    }
                }
            };

            let mut spans = Vec::new();
            let mut column = 0;
            match ops {
                Some(ops) => {
                    for (style, piece) in
                        HighlightIterator::new(&mut self.highlight, &ops, line, &theme)
                    {
                        push_span(&mut spans, &mut column, piece, highlighter.style(style));
                    }
                }
                None => push_span(&mut spans, &mut column, line, Style::new()),
            }
            self.lines.push(Line::from(spans));
        }
        &self.lines[..count.min(self.lines.len())]
    }
}

/// Loading bat's syntaxes takes a moment, so tests share one highlighter.
#[cfg(test)]
pub fn test_highlighter() -> &'static Highlighter {
    static HIGHLIGHTER: std::sync::OnceLock<Highlighter> = std::sync::OnceLock::new();
    HIGHLIGHTER.get_or_init(|| Highlighter::new(DEFAULT_THEME, Colors::TrueColor))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn syntax(text: &str, source: Option<&str>) -> String {
        test_highlighter()
            .syntax_for(text, source.map(Path::new))
            .name
            .clone()
    }

    fn shown(pieces: &[&str]) -> String {
        let mut spans = Vec::new();
        let mut column = 0;
        for piece in pieces {
            push_span(&mut spans, &mut column, piece, Style::new());
        }
        spans.iter().map(|span| span.content.as_ref()).collect()
    }

    #[test]
    fn syntax_comes_from_the_source_file() {
        for (path, expected) in [
            ("src/main.rs", "Rust"),
            ("Cargo.toml", "TOML"),
            ("Dockerfile", "Dockerfile"),
            ("notes.txt", "Plain Text"),
        ] {
            assert_eq!(syntax("", Some(path)), expected, "{}", path);
        }
    }

    #[test]
    fn syntax_is_guessed_from_the_text() {
        for (text, expected) in [
            ("#!/bin/bash\necho hi", "Bourne Again Shell (bash)"),
            ("<?xml version=\"1.0\"?>\n<a/>", "XML"),
            ("diff --git a/x b/x\n--- a/x\n+++ b/x", "Diff"),
            ("  {\"a\": [1, 2]}\n", "JSON"),
            ("{ not json", "Plain Text"),
            ("just some words", "Plain Text"),
        ] {
            assert_eq!(syntax(text, None), expected, "{}", text);
        }
    }

    #[test]
    fn the_source_file_wins_over_the_text() {
        assert_eq!(syntax("{\"a\": 1}", Some("x.rs")), "Rust");
    }

    #[test]
    fn previews_are_highlighted_lazily() {
        let text: String = (0..100).map(|i| format!("let x{} = {};\n", i, i)).collect();
        let mut preview = Preview::new(test_highlighter(), &text, Some(Path::new("a.rs")));
        assert_eq!(preview.line_count, 100);
        assert_eq!(preview.lines(test_highlighter(), 5).len(), 5);
        assert_eq!(preview.lines.len(), 5);
        assert_eq!(preview.lines(test_highlighter(), 500).len(), 100);
    }

    #[test]
    fn code_gets_colors_from_the_theme() {
        let mut preview = Preview::new(
            test_highlighter(),
            "fn main() {}\n",
            Some(Path::new("a.rs")),
        );
        let line = &preview.lines(test_highlighter(), 1)[0];
        assert_eq!(line.to_string(), "fn main() {}");
        let colors: Vec<_> = line.spans.iter().filter_map(|span| span.style.fg).collect();
        assert!(colors.len() > 1, "{:?}", line);
        assert!(colors.iter().all(|color| matches!(color, Color::Rgb(..))));
    }

    #[test]
    fn overlong_lines_are_shown_plain() {
        let text = format!("[{}]", "1,".repeat(MAX_HIGHLIGHTED_LINE_BYTES));
        let mut preview = Preview::new(test_highlighter(), &text, Some(Path::new("a.json")));
        let line = &preview.lines(test_highlighter(), 1)[0];
        assert_eq!(line.spans.len(), 1);
        assert_eq!(line.spans[0].style, Style::new());
    }

    #[test]
    fn control_characters_never_reach_the_terminal() {
        assert_eq!(
            shown(&["a\u{1b}[31mb\u{7}c\u{9b}\r\n"]),
            "a\u{fffd}[31mb\u{fffd}c\u{fffd}"
        );
    }

    #[test]
    fn tabs_expand_to_tab_stops_across_spans() {
        assert_eq!(shown(&["\tx"]), "    x");
        assert_eq!(shown(&["ab", "\tc"]), "ab  c");
    }

    #[test]
    fn palette_colors_follow_bat() {
        let palette = |r, a| syntect::highlighting::Color { r, g: 0, b: 0, a };
        assert_eq!(
            convert_color(palette(1, 0), Colors::TrueColor),
            Some(Color::Red)
        );
        assert_eq!(
            convert_color(palette(7, 0), Colors::Ansi256),
            Some(Color::Gray)
        );
        assert_eq!(
            convert_color(palette(42, 0), Colors::TrueColor),
            Some(Color::Indexed(42))
        );
        assert_eq!(convert_color(palette(1, 1), Colors::TrueColor), None);
        assert_eq!(convert_color(palette(200, 255), Colors::None), None);
    }

    #[test]
    fn rgb_maps_onto_the_256_color_palette() {
        assert_eq!(ansi256(0, 0, 0), 16);
        assert_eq!(ansi256(255, 255, 255), 231);
        assert_eq!(ansi256(128, 128, 128), 244);
        assert_eq!(ansi256(255, 0, 0), 196);
        assert_eq!(ansi256(0, 0, 255), 21);
    }

    #[test]
    fn themes_are_found_by_name_ignoring_case() {
        assert_eq!(
            find_theme("monokai extended"),
            Some(EmbeddedThemeName::MonokaiExtended)
        );
        assert_eq!(find_theme("ansi"), Some(EmbeddedThemeName::Ansi));
        assert_eq!(find_theme("no such theme"), None);
    }
}
