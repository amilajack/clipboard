//! `cb peek`: search clipboard history, with a syntax-highlighted preview.

use std::io;
use std::ops::Range;
use std::path::Path;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph};
use ratatui::{DefaultTerminal, Frame};

use crate::highlight::{push_span, Highlighter, Preview};
use crate::history::{self, Entry};

/// Lines kept above a search match when the preview scrolls to it.
const MATCH_CONTEXT: usize = 3;

/// Narrower than this, the list goes above the preview instead of beside it.
const SIDE_BY_SIDE_WIDTH: u16 = 80;

const FAINT: Style = Style::new().fg(Color::DarkGray);
const MATCH: Style = Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD);
const PROMPT: Style = Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD);

/// Lets the user pick an entry from `entries`, given oldest first. Returns
/// `None` if they quit without picking one.
pub fn run(entries: Vec<Entry>, highlighter: &Highlighter) -> io::Result<Option<Entry>> {
    let mut app = App::new(entries, highlighter, history::now());
    let mut terminal = match ratatui::try_init() {
        Ok(terminal) => terminal,
        Err(e) => {
            ratatui::restore();
            return Err(e);
        }
    };
    let picked = app.event_loop(&mut terminal);
    ratatui::restore();
    Ok(picked?.map(|index| app.entries.swap_remove(index)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Match {
    entry: usize,
    /// The line to show in the list: the first one that matches the search,
    /// or with no search, the first one that isn't blank.
    line: usize,
}

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Continue,
    Quit,
    Copy(usize),
}

struct App<'a> {
    /// The most frecent first.
    entries: Vec<Entry>,
    /// Lowercased copies of `entries`, for case-insensitive search.
    lowercase: Vec<String>,
    query: String,
    matches: Vec<Match>,
    list: ListState,
    /// The first preview line on screen.
    scroll: usize,
    /// Lines of preview on screen at the last draw, for paging.
    preview_height: usize,
    /// The preview of the selected entry, kept while it stays selected.
    preview: Option<(usize, Preview)>,
    highlighter: &'a Highlighter,
    now: u64,
}

impl<'a> App<'a> {
    fn new(mut entries: Vec<Entry>, highlighter: &'a Highlighter, now: u64) -> Self {
        // The most frecent first, and of two equals, the newer.
        entries.reverse();
        entries.sort_by(|a, b| b.frecency(now).total_cmp(&a.frecency(now)));
        let lowercase = entries
            .iter()
            .map(|entry| entry.text.to_lowercase())
            .collect();
        let mut app = Self {
            entries,
            lowercase,
            query: String::new(),
            matches: Vec::new(),
            list: ListState::default(),
            scroll: 0,
            preview_height: 0,
            preview: None,
            highlighter,
            now,
        };
        app.filter();
        app
    }

    fn event_loop(&mut self, terminal: &mut DefaultTerminal) -> io::Result<Option<usize>> {
        loop {
            terminal.draw(|frame| self.render(frame))?;
            // Anything else, like a resize, only needs the redraw.
            let Event::Key(key) = event::read()? else {
                continue;
            };
            if key.kind == KeyEventKind::Release {
                continue;
            }
            match self.on_key(key) {
                Outcome::Continue => {}
                Outcome::Quit => return Ok(None),
                Outcome::Copy(index) => return Ok(Some(index)),
            }
        }
    }

    fn on_key(&mut self, key: KeyEvent) -> Outcome {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let page = self.preview_height.max(1) as isize;
        match key.code {
            KeyCode::Esc => return Outcome::Quit,
            KeyCode::Char('c' | 'g') if ctrl => return Outcome::Quit,
            KeyCode::Enter => {
                if let Some(selected) = self.selected() {
                    return Outcome::Copy(selected.entry);
                }
            }
            KeyCode::Up if shift => self.scroll_by(-1),
            KeyCode::Down if shift => self.scroll_by(1),
            KeyCode::Up => self.move_by(-1),
            KeyCode::Down => self.move_by(1),
            // As in a shell's history search, Ctrl-R moves on to the next
            // match and Ctrl-S back to the previous one.
            KeyCode::Char('r' | 'n') if ctrl => self.move_by(1),
            KeyCode::Char('s' | 'p') if ctrl => self.move_by(-1),
            KeyCode::PageUp => self.scroll_by(-page),
            KeyCode::PageDown => self.scroll_by(page),
            KeyCode::Backspace => {
                if self.query.pop().is_some() {
                    self.filter();
                }
            }
            KeyCode::Char('u') if ctrl => {
                self.query.clear();
                self.filter();
            }
            KeyCode::Char('w') if ctrl => {
                let kept = self
                    .query
                    .trim_end()
                    .trim_end_matches(|c: char| !c.is_whitespace())
                    .len();
                self.query.truncate(kept);
                self.filter();
            }
            // AltGr arrives as Ctrl+Alt on Windows.
            KeyCode::Char(c) if !ctrl || key.modifiers.contains(KeyModifiers::ALT) => {
                self.query.push(c);
                self.filter();
            }
            _ => {}
        }
        Outcome::Continue
    }

    /// Smart case, as in ripgrep and fzf: an uppercase letter in the search
    /// makes it case-sensitive.
    fn case_sensitive(&self) -> bool {
        self.query.chars().any(char::is_uppercase)
    }

    /// Finds the entries that match the search, keeping them in frecency
    /// order, and selects the first.
    fn filter(&mut self) {
        let case_sensitive = self.case_sensitive();
        let query = if case_sensitive {
            self.query.clone()
        } else {
            self.query.to_lowercase()
        };
        self.matches = (self.entries.iter().zip(&self.lowercase))
            .enumerate()
            .filter_map(|(entry, (original, lowercase))| {
                let line = if query.is_empty() {
                    first_nonblank_line(&original.text)
                } else {
                    // Lowercasing never adds or removes a newline, so line
                    // numbers in the lowercase copy hold for the original.
                    let text = if case_sensitive {
                        &original.text
                    } else {
                        lowercase
                    };
                    let at = text.find(&query)?;
                    text[..at].matches('\n').count()
                };
                Some(Match { entry, line })
            })
            .collect();
        self.select(0);
    }

    fn selected(&self) -> Option<&Match> {
        self.list
            .selected()
            .and_then(|index| self.matches.get(index))
    }

    fn select(&mut self, index: usize) {
        let Some(last) = self.matches.len().checked_sub(1) else {
            self.list.select(None);
            return;
        };
        let index = index.min(last);
        self.list.select(Some(index));
        // Show where the search matched, with a little context above it.
        self.scroll = if self.query.is_empty() {
            0
        } else {
            self.matches[index].line.saturating_sub(MATCH_CONTEXT)
        };
    }

    fn move_by(&mut self, delta: isize) {
        if let Some(index) = self.list.selected() {
            self.select(index.saturating_add_signed(delta));
        }
    }

    fn scroll_by(&mut self, delta: isize) {
        let line_count = self
            .preview
            .as_ref()
            .map_or(0, |(_, preview)| preview.line_count);
        let max = line_count.saturating_sub(self.preview_height);
        self.scroll = self.scroll.saturating_add_signed(delta).min(max);
    }

    fn render(&mut self, frame: &mut Frame) {
        let [main, prompt] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(frame.area());
        let panes = [Constraint::Percentage(40), Constraint::Percentage(60)];
        let [list, preview] = if main.width >= SIDE_BY_SIDE_WIDTH {
            Layout::horizontal(panes).areas(main)
        } else {
            Layout::vertical(panes).areas(main)
        };
        self.render_list(frame, list);
        self.render_preview(frame, preview);
        self.render_prompt(frame, prompt);
    }

    fn render_list(&mut self, frame: &mut Frame, area: Rect) {
        let items: Vec<_> = self.matches.iter().map(|m| self.row(m)).collect();
        let count = format!(" {}/{} ", self.matches.len(), self.entries.len());
        let block = Block::bordered()
            .border_style(FAINT)
            .title_top(" History ")
            .title_top(Line::from(count).right_aligned());
        let list = List::new(items)
            .block(block)
            .highlight_style(Style::new().add_modifier(Modifier::REVERSED));
        frame.render_stateful_widget(list, area, &mut self.list);
    }

    /// An entry in the list: its age, then the line that matched the search
    /// with the match picked out.
    fn row(&self, m: &Match) -> ListItem<'static> {
        let entry = &self.entries[m.entry];
        let line = entry.text.lines().nth(m.line).unwrap_or_default().trim();
        let mut spans = vec![Span::styled(
            format!("{:>4} ", age(self.now, entry.time)),
            FAINT,
        )];
        let mut column = 0;
        if line.is_empty() {
            spans.push(Span::styled("(blank)", FAINT));
        } else if let Some(found) = find(line, &self.query, self.case_sensitive()) {
            push_span(&mut spans, &mut column, &line[..found.start], Style::new());
            push_span(&mut spans, &mut column, &line[found.clone()], MATCH);
            push_span(&mut spans, &mut column, &line[found.end..], Style::new());
        } else {
            push_span(&mut spans, &mut column, line, Style::new());
        }
        ListItem::new(Line::from(spans))
    }

    fn render_preview(&mut self, frame: &mut Frame, area: Rect) {
        let block = Block::bordered().border_style(FAINT);
        let height = block.inner(area).height as usize;
        self.preview_height = height;
        let Some(&selected) = self.selected() else {
            let empty = Paragraph::new(Line::styled("No matches", FAINT));
            frame.render_widget(empty.block(block), area);
            return;
        };

        let entry = &self.entries[selected.entry];
        if self
            .preview
            .as_ref()
            .is_none_or(|(shown, _)| *shown != selected.entry)
        {
            let preview = Preview::new(self.highlighter, &entry.text, entry.source.as_deref());
            self.preview = Some((selected.entry, preview));
        }
        let (_, preview) = self.preview.as_mut().expect("the preview was just made");

        let mut title = Vec::new();
        if let Some(name) = entry.source.as_deref().and_then(Path::file_name) {
            title.push(name.to_string_lossy().into_owned());
        }
        title.push(preview.syntax.clone());
        title.push(match preview.line_count {
            1 => "1 line".to_owned(),
            n => format!("{} lines", n),
        });
        if entry.uses > 1 {
            title.push(format!("copied {} times", entry.uses));
        }
        let mut title_spans = Vec::new();
        push_span(
            &mut title_spans,
            &mut 0,
            &format!(" {} ", title.join(" · ")),
            Style::new(),
        );

        self.scroll = self.scroll.min(preview.line_count.saturating_sub(height));
        let number_width = preview.line_count.to_string().len();
        let searching = !self.query.is_empty();
        let lines: Vec<_> = preview
            .lines(self.highlighter, self.scroll + height)
            .iter()
            .enumerate()
            .skip(self.scroll)
            .map(|(number, line)| {
                let style = if searching && number == selected.line {
                    MATCH
                } else {
                    FAINT
                };
                let gutter = format!("{:>width$} │ ", number + 1, width = number_width);
                let mut spans = vec![Span::styled(gutter, style)];
                spans.extend(line.spans.iter().cloned());
                Line::from(spans)
            })
            .collect();
        let block = block.title_top(Line::from(title_spans));
        frame.render_widget(Paragraph::new(lines).block(block), area);
    }

    fn render_prompt(&self, frame: &mut Frame, area: Rect) {
        let mut spans = vec![Span::styled("> ", PROMPT)];
        push_span(&mut spans, &mut 0, &self.query, Style::new());
        let prompt = Line::from(spans);
        let cursor = prompt.width() as u16;
        let hints =
            Line::styled("↑↓ select  ⏎ copy  PgUp/PgDn scroll  Esc quit ", FAINT).right_aligned();
        if prompt.width() + hints.width() < area.width as usize {
            frame.render_widget(hints, area);
        }
        frame.render_widget(prompt, area);
        frame.set_cursor_position((area.x + cursor.min(area.width.saturating_sub(1)), area.y));
    }
}

fn first_nonblank_line(text: &str) -> usize {
    text.lines()
        .position(|line| !line.trim().is_empty())
        .unwrap_or(0)
}

/// How long ago `then` was, in a few characters.
fn age(now: u64, then: u64) -> String {
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;
    const YEAR: u64 = 365 * DAY;
    let seconds = now.saturating_sub(then);
    match seconds {
        0..MINUTE => "now".to_owned(),
        MINUTE..HOUR => format!("{}m", seconds / MINUTE),
        HOUR..DAY => format!("{}h", seconds / HOUR),
        DAY..YEAR => format!("{}d", seconds / DAY),
        _ => format!("{}y", seconds / YEAR),
    }
}

/// Where `needle` first appears in `haystack`, as a byte range of `haystack`.
/// Without `case_sensitive`, letters are compared by their lowercase forms,
/// char by char, so the range stays right even where lowercasing changes how
/// many bytes a char takes.
fn find(haystack: &str, needle: &str, case_sensitive: bool) -> Option<Range<usize>> {
    if needle.is_empty() {
        return None;
    }
    if case_sensitive {
        return haystack
            .find(needle)
            .map(|start| start..start + needle.len());
    }
    let needle: Vec<char> = needle.chars().flat_map(char::to_lowercase).collect();
    haystack.char_indices().find_map(|(start, _)| {
        let mut wanted = needle.iter();
        for (offset, c) in haystack[start..].char_indices() {
            let end = start + offset + c.len_utf8();
            for lower in c.to_lowercase() {
                match wanted.next() {
                    Some(&want) if want == lower => {}
                    Some(_) => return None,
                    None => return Some(start..end),
                }
            }
            if wanted.as_slice().is_empty() {
                return Some(start..end);
            }
        }
        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::highlight::test_highlighter;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    const NOW: u64 = 1_000_000;

    /// An app over `texts`, given oldest first as history keeps them, copied a
    /// minute apart.
    fn app(texts: &[&str]) -> App<'static> {
        let entries = texts
            .iter()
            .enumerate()
            .map(|(i, text)| Entry {
                text: (*text).to_owned(),
                source: None,
                time: NOW - 60 * (texts.len() - i) as u64,
                uses: 1,
                score: 1.0,
            })
            .collect();
        App::new(entries, test_highlighter(), NOW)
    }

    fn press(app: &mut App, code: KeyCode) -> Outcome {
        app.on_key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn ctrl(app: &mut App, c: char) -> Outcome {
        app.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
    }

    fn type_text(app: &mut App, text: &str) {
        for c in text.chars() {
            assert_eq!(press(app, KeyCode::Char(c)), Outcome::Continue);
        }
    }

    fn matched<'a>(app: &'a App) -> Vec<&'a str> {
        (app.matches.iter())
            .map(|m| app.entries[m.entry].text.as_str())
            .collect()
    }

    fn selected<'a>(app: &'a App) -> &'a str {
        &app.entries[app.selected().expect("something is selected").entry].text
    }

    fn render(app: &mut App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect())
            .collect::<Vec<String>>()
            .join("\n")
    }

    #[test]
    fn newest_entry_comes_first_and_is_selected() {
        let app = app(&["old", "new"]);
        assert_eq!(matched(&app), ["new", "old"]);
        assert_eq!(selected(&app), "new");
    }

    #[test]
    fn arrows_move_the_selection_and_stop_at_the_ends() {
        let mut app = app(&["old", "new"]);
        press(&mut app, KeyCode::Up);
        assert_eq!(selected(&app), "new");
        press(&mut app, KeyCode::Down);
        assert_eq!(selected(&app), "old");
        press(&mut app, KeyCode::Down);
        assert_eq!(selected(&app), "old");
    }

    #[test]
    fn typing_filters_with_smart_case() {
        let mut app = app(&["Hello world", "hello there", "bye"]);
        type_text(&mut app, "hello");
        assert_eq!(matched(&app), ["hello there", "Hello world"]);
        ctrl(&mut app, 'u');
        type_text(&mut app, "Hello");
        assert_eq!(matched(&app), ["Hello world"]);
    }

    #[test]
    fn ctrl_r_steps_back_through_matches() {
        let mut app = app(&["git push", "ls", "git status"]);
        type_text(&mut app, "git");
        assert_eq!(selected(&app), "git status");
        ctrl(&mut app, 'r');
        assert_eq!(selected(&app), "git push");
        ctrl(&mut app, 's');
        assert_eq!(selected(&app), "git status");
    }

    #[test]
    fn search_matches_any_line_and_scrolls_the_preview_to_it() {
        let text: String = (0..20)
            .map(|i| {
                if i == 12 {
                    "the needle\n".to_owned()
                } else {
                    format!("line {}\n", i)
                }
            })
            .collect();
        let mut app = app(&[&text]);
        type_text(&mut app, "needle");
        assert_eq!(app.matches, [Match { entry: 0, line: 12 }]);
        assert_eq!(app.scroll, 12 - MATCH_CONTEXT);
        assert!(render(&mut app, 100, 12).contains("the needle"));
    }

    #[test]
    fn enter_copies_the_selection_and_escape_quits() {
        let mut app = app(&["old", "new"]);
        press(&mut app, KeyCode::Down);
        assert_eq!(press(&mut app, KeyCode::Enter), Outcome::Copy(1));
        assert_eq!(press(&mut app, KeyCode::Esc), Outcome::Quit);
        assert_eq!(ctrl(&mut app, 'c'), Outcome::Quit);
        type_text(&mut app, "nothing matches this");
        assert_eq!(press(&mut app, KeyCode::Enter), Outcome::Continue);
    }

    #[test]
    fn the_search_can_be_edited() {
        let mut app = app(&["x"]);
        type_text(&mut app, "foo bar");
        ctrl(&mut app, 'w');
        assert_eq!(app.query, "foo ");
        press(&mut app, KeyCode::Backspace);
        assert_eq!(app.query, "foo");
        ctrl(&mut app, 'u');
        assert_eq!(app.query, "");
        press(&mut app, KeyCode::Backspace);
        assert_eq!(matched(&app), ["x"]);
    }

    #[test]
    fn renders_the_list_beside_a_numbered_preview() {
        let mut app = App::new(
            vec![Entry {
                text: "fn main() {\n    println!(\"hi\");\n}".to_owned(),
                source: Some("/src/main.rs".into()),
                time: NOW - 120,
                uses: 1,
                score: 1.0,
            }],
            test_highlighter(),
            NOW,
        );
        let screen = render(&mut app, 100, 10);
        assert!(screen.contains("History"), "{}", screen);
        assert!(screen.contains("1/1"), "{}", screen);
        assert!(screen.contains("  2m fn main() {"), "{}", screen);
        assert!(screen.contains("main.rs · Rust · 3 lines"), "{}", screen);
        assert!(screen.contains("1 │ fn main() {"), "{}", screen);
        assert!(screen.contains("3 │ }"), "{}", screen);
    }

    #[test]
    fn narrow_terminals_stack_the_panes() {
        let mut app = app(&["hello"]);
        let screen = render(&mut app, 40, 12);
        let row = |needle| screen.lines().position(|line| line.contains(needle));
        assert!(row("History") < row("Plain Text"), "{}", screen);
    }

    #[test]
    fn says_when_nothing_matches() {
        let mut app = app(&["hello"]);
        type_text(&mut app, "zzz");
        let screen = render(&mut app, 100, 10);
        assert!(screen.contains("No matches"), "{}", screen);
        assert!(screen.contains("0/1"), "{}", screen);
    }

    #[test]
    fn escape_sequences_in_entries_are_shown_harmlessly() {
        let mut app = app(&["\u{1b}[2Jgotcha"]);
        let screen = render(&mut app, 100, 10);
        assert!(!screen.contains('\u{1b}'));
        assert!(screen.contains("\u{fffd}[2Jgotcha"), "{}", screen);
    }

    /// An entry used `uses` times, last `idle` seconds ago, each use then.
    fn used(text: &str, uses: u32, idle: u64) -> Entry {
        Entry {
            text: text.to_owned(),
            source: None,
            time: NOW - idle,
            uses,
            score: uses.into(),
        }
    }

    #[test]
    fn entries_used_often_come_before_newer_ones() {
        let app = App::new(
            vec![used("favorite", 5, 3600), used("once", 1, 60)],
            test_highlighter(),
            NOW,
        );
        assert_eq!(matched(&app), ["favorite", "once"]);
        assert_eq!(selected(&app), "favorite");
    }

    #[test]
    fn favorites_sink_once_they_go_unused() {
        let app = App::new(
            vec![used("old favorite", 20, 10 * 86400), used("once", 1, 60)],
            test_highlighter(),
            NOW,
        );
        assert_eq!(matched(&app), ["once", "old favorite"]);
    }

    #[test]
    fn matches_stay_in_frecency_order() {
        let mut app = App::new(
            vec![
                used("git push", 4, 3600),
                used("ls", 9, 60),
                used("git status", 1, 60),
            ],
            test_highlighter(),
            NOW,
        );
        type_text(&mut app, "git");
        assert_eq!(matched(&app), ["git push", "git status"]);
    }

    #[test]
    fn the_preview_says_how_often_an_entry_was_copied() {
        let mut app = App::new(vec![used("hello", 3, 60)], test_highlighter(), NOW);
        let screen = render(&mut app, 100, 10);
        assert!(screen.contains("copied 3 times"), "{}", screen);
    }

    #[test]
    fn ages_are_short() {
        assert_eq!(age(NOW, NOW), "now");
        assert_eq!(age(NOW, NOW - 59), "now");
        assert_eq!(age(NOW, NOW - 60), "1m");
        assert_eq!(age(NOW, NOW - 2 * 3600), "2h");
        assert_eq!(age(NOW, NOW - 3 * 86400), "3d");
        assert_eq!(age(NOW + 400 * 86400, NOW), "1y");
        assert_eq!(age(NOW, NOW + 10), "now");
    }

    #[test]
    fn find_ignores_case_across_multibyte_text() {
        let text = "Grüße, WORLD";
        let found = find(text, "world", false).unwrap();
        assert_eq!(&text[found], "WORLD");
        assert_eq!(find("ÄBC", "äb", false), Some(0..3));
        assert_eq!(find("abc", "B", true), None);
        assert_eq!(find("abc", "", false), None);
    }
}
