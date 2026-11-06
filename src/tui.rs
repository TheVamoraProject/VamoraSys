//! A small keyboard-driven terminal UI for VamoraSys, in the style of Vaminfo's config wizard:
//! boxed title, `▶` marker, dimmed hint line, `n/m` counter. Arrow keys / PageUp / PageDown / Home / End,
//! Enter to choose, Esc to go back, and type-to-search on long lists. Built on termios from `libc`
//! so it adds no dependency. Colors come from `theme` (the Settings accent, nearest color on limited terminals).

use crate::theme::{self, Rgb};
use std::io::{self, IsTerminal, Write};
use std::time::Duration;

const DIM: Rgb = Rgb(100, 116, 139);

/// Both ends must be a terminal: scripts and apps calling VamoraSys never get a prompt.
pub fn is_interactive() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

// ───────────────────────────── keys ─────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
    Enter,
    Esc,
    Backspace,
    Delete,
    Tab,
    CtrlC,
    Char(char),
    Unknown,
}

/// Decodes the first key in `bytes`; returns it and how many bytes it used. Pure, so it can be tested.
pub fn parse_key(bytes: &[u8]) -> (Key, usize) {
    let Some(&first) = bytes.first() else {
        return (Key::Unknown, 0);
    };
    match first {
        0x1b => parse_escape(bytes),
        b'\r' | b'\n' => (Key::Enter, 1),
        0x7f | 0x08 => (Key::Backspace, 1),
        b'\t' => (Key::Tab, 1),
        0x03 => (Key::CtrlC, 1),
        0x04 => (Key::Esc, 1), // Ctrl-D
        0x20..=0x7e => (Key::Char(first as char), 1),
        0x00..=0x1f => (Key::Unknown, 1),
        _ => {
            let width = match first {
                0xc0..=0xdf => 2,
                0xe0..=0xef => 3,
                0xf0..=0xf7 => 4,
                _ => 1,
            };
            let width = width.min(bytes.len());
            match std::str::from_utf8(&bytes[..width])
                .ok()
                .and_then(|s| s.chars().next())
            {
                Some(character) => (Key::Char(character), width),
                None => (Key::Unknown, width),
            }
        }
    }
}

fn parse_escape(bytes: &[u8]) -> (Key, usize) {
    match bytes.get(1) {
        None => (Key::Esc, 1),
        Some(b'[') | Some(b'O') => {
            let mut end = 2;
            while end < bytes.len() && !(0x40..=0x7e).contains(&bytes[end]) {
                end += 1;
            }
            if end >= bytes.len() {
                return (Key::Unknown, bytes.len());
            }
            let used = end + 1;
            let params = &bytes[2..end];
            let key = match bytes[end] {
                b'A' => Key::Up,
                b'B' => Key::Down,
                b'C' => Key::Right,
                b'D' => Key::Left,
                b'H' => Key::Home,
                b'F' => Key::End,
                b'~' => match params {
                    b"1" | b"7" => Key::Home,
                    b"4" | b"8" => Key::End,
                    b"3" => Key::Delete,
                    b"5" => Key::PageUp,
                    b"6" => Key::PageDown,
                    _ => Key::Unknown,
                },
                _ => Key::Unknown,
            };
            (key, used)
        }
        // Alt+key: treat as a plain Esc and let the next key be read normally
        Some(_) => (Key::Esc, 1),
    }
}

// ───────────────────────────── list logic (pure) ─────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    pub detail: String,
}

impl Item {
    pub fn new(label: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            detail: detail.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Pending,
    Chosen(usize),
    Cancelled,
}

#[derive(Clone, Debug, Default)]
pub struct ListState {
    pub filter: String,
    /// Position inside the filtered list.
    pub selected: usize,
    pub top: usize,
}

/// Indexes of the items that match `filter` (case-insensitive, in label and detail).
pub fn matching(items: &[Item], filter: &str) -> Vec<usize> {
    let needle = filter.trim().to_lowercase();
    items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            needle.is_empty()
                || format!("{} {}", item.label, item.detail)
                    .to_lowercase()
                    .contains(&needle)
        })
        .map(|(index, _)| index)
        .collect()
}

impl ListState {
    pub fn starting_at(item: usize) -> Self {
        Self {
            selected: item,
            ..Default::default()
        }
    }

    pub fn scroll_to_selection(&mut self, rows: usize) {
        let rows = rows.max(1);
        if self.selected < self.top {
            self.top = self.selected;
        }
        if self.selected >= self.top + rows {
            self.top = self.selected + 1 - rows;
        }
    }

    pub fn apply(&mut self, key: Key, items: &[Item], rows: usize, filterable: bool) -> Outcome {
        let visible = matching(items, &self.filter);
        let last = visible.len().saturating_sub(1);
        match key {
            Key::CtrlC | Key::Esc => return Outcome::Cancelled,
            Key::Char('q') | Key::Char('Q') if !filterable => return Outcome::Cancelled,
            Key::Enter => {
                if let Some(&index) = visible.get(self.selected) {
                    return Outcome::Chosen(index);
                }
            }
            Key::Char(' ') if !filterable => {
                if let Some(&index) = visible.get(self.selected) {
                    return Outcome::Chosen(index);
                }
            }
            Key::Up => self.selected = self.selected.saturating_sub(1),
            Key::Char('k') if !filterable => self.selected = self.selected.saturating_sub(1),
            Key::Down => self.selected = (self.selected + 1).min(last),
            Key::Char('j') if !filterable => self.selected = (self.selected + 1).min(last),
            Key::PageUp => self.selected = self.selected.saturating_sub(rows.max(1)),
            Key::PageDown => self.selected = (self.selected + rows.max(1)).min(last),
            Key::Home => self.selected = 0,
            Key::End => self.selected = last,
            Key::Char('g') if !filterable => self.selected = 0,
            Key::Char('G') if !filterable => self.selected = last,
            Key::Backspace if filterable => {
                self.filter.pop();
                self.selected = 0;
                self.top = 0;
            }
            Key::Char(character) if filterable => {
                self.filter.push(character);
                self.selected = 0;
                self.top = 0;
            }
            _ => {}
        }
        self.selected = self.selected.min(last);
        self.scroll_to_selection(rows);
        Outcome::Pending
    }
}

// ───────────────────────────── raw terminal ─────────────────────────────

/// Raw mode + alternate screen while alive; restores the terminal when dropped (also on panic).
pub struct Terminal {
    original: libc::termios,
    pending: Vec<u8>,
}

impl Terminal {
    pub fn enter() -> io::Result<Self> {
        // SAFETY: plain libc termios calls on stdin with a zeroed struct we own.
        let original = unsafe {
            let mut termios: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(libc::STDIN_FILENO, &mut termios) != 0 {
                return Err(io::Error::last_os_error());
            }
            termios
        };
        let mut raw = original;
        unsafe {
            libc::cfmakeraw(&mut raw);
            raw.c_cc[libc::VMIN] = 1;
            raw.c_cc[libc::VTIME] = 0;
            if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, &raw) != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        let mut out = io::stdout();
        out.write_all(b"\x1b[?1049h\x1b[?25l")?; // alternate screen, hide cursor
        out.flush()?;
        Ok(Self {
            original,
            pending: Vec::new(),
        })
    }

    pub fn size(&self) -> (usize, usize) {
        // SAFETY: TIOCGWINSZ fills a winsize we own.
        let mut size: libc::winsize = unsafe { std::mem::zeroed() };
        let ok = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut size) } == 0;
        if ok && size.ws_row > 0 && size.ws_col > 0 {
            (size.ws_row as usize, size.ws_col as usize)
        } else {
            (24, 80)
        }
    }

    fn fill(&mut self, wait: Option<Duration>) -> bool {
        if let Some(wait) = wait {
            let mut poll = libc::pollfd {
                fd: libc::STDIN_FILENO,
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: one valid pollfd.
            let ready = unsafe { libc::poll(&mut poll, 1, wait.as_millis() as i32) };
            if ready <= 0 {
                return false;
            }
        }
        let mut buffer = [0u8; 64];
        // SAFETY: reads at most buffer.len() bytes into our buffer.
        let count =
            unsafe { libc::read(libc::STDIN_FILENO, buffer.as_mut_ptr().cast(), buffer.len()) };
        if count <= 0 {
            return false;
        }
        self.pending.extend_from_slice(&buffer[..count as usize]);
        true
    }

    /// Blocks until a key is pressed. End of input (the terminal went away) counts as Ctrl-C.
    pub fn read_key(&mut self) -> Key {
        if self.pending.is_empty() && !self.fill(None) {
            return Key::CtrlC;
        }
        // A lone Esc may be the start of an arrow key whose bytes have not all arrived yet.
        if self.pending == [0x1b] {
            self.fill(Some(Duration::from_millis(40)));
        }
        let (key, used) = parse_key(&self.pending);
        self.pending.drain(..used.max(1).min(self.pending.len()));
        key
    }

    pub fn draw(&self, lines: &[String]) {
        let mut out = io::stdout();
        let mut frame = String::from("\x1b[H\x1b[2J");
        for line in lines {
            frame.push_str(line);
            frame.push_str("\r\n");
        }
        let _ = out.write_all(frame.as_bytes());
        let _ = out.flush();
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let mut out = io::stdout();
        let _ = out.write_all(b"\x1b[?25h\x1b[?1049l"); // show cursor, leave alternate screen
        let _ = out.flush();
        // SAFETY: restores the termios we saved in `enter`.
        unsafe {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, &self.original);
        }
    }
}

// ───────────────────────────── rendering ─────────────────────────────

struct Style {
    accent: String,
    dim: String,
    bold: &'static str,
    reset: &'static str,
}

impl Style {
    fn new() -> Self {
        let colored = theme::support() != theme::ColorSupport::None;
        Self {
            accent: theme::accent_for_terminal(),
            dim: theme::fg(DIM, theme::support()),
            bold: if colored { "\x1b[1m" } else { "" },
            reset: if colored { "\x1b[0m" } else { "" },
        }
    }
}

fn header(title: &str, style: &Style) -> Vec<String> {
    let width = title.chars().count() + 4;
    let line = |text: String| format!("{}{}{text}{}", style.bold, style.accent, style.reset);
    vec![
        String::new(),
        line(format!("  ╔{}╗", "═".repeat(width))),
        line(format!("  ║  {title}  ║")),
        line(format!("  ╚{}╝", "═".repeat(width))),
        String::new(),
    ]
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let mut shortened: String = text.chars().take(max.saturating_sub(1)).collect();
        shortened.push('…');
        shortened
    }
}

pub struct Screen<'a> {
    pub title: &'a str,
    pub hint: &'a str,
    /// Lines shown between the hint and the list (what was detected, what will be written, ...).
    pub info: &'a [String],
    pub filterable: bool,
}

/// Shows a list and returns the chosen item's index, or `None` when the user backed out.
pub fn select(
    terminal: &mut Terminal,
    screen: &Screen,
    items: &[Item],
    initial: usize,
) -> Option<usize> {
    let style = Style::new();
    let mut state = ListState::starting_at(initial.min(items.len().saturating_sub(1)));
    loop {
        let (rows, columns) = terminal.size();
        let chrome = 9
            + screen.info.len()
            + usize::from(!screen.info.is_empty())
            + usize::from(screen.filterable);
        let visible_rows = rows.saturating_sub(chrome).max(3);
        state.scroll_to_selection(visible_rows);
        let visible = matching(items, &state.filter);

        let mut lines = header(screen.title, &style);
        lines.push(format!("  {}{}{}", style.dim, screen.hint, style.reset));
        lines.push(String::new());
        for info in screen.info {
            lines.push(format!("  {}", truncate(info, columns.saturating_sub(4))));
        }
        if !screen.info.is_empty() {
            lines.push(String::new());
        }
        if screen.filterable {
            let shown = if state.filter.is_empty() {
                format!("{}type to search{}", style.dim, style.reset)
            } else {
                state.filter.clone()
            };
            lines.push(format!("  {}search:{} {shown}", style.accent, style.reset));
            lines.push(String::new());
        }

        let label_width = visible
            .iter()
            .skip(state.top)
            .take(visible_rows)
            .map(|&index| items[index].label.chars().count())
            .max()
            .unwrap_or(0)
            .min(34);
        for (position, &index) in visible
            .iter()
            .enumerate()
            .skip(state.top)
            .take(visible_rows)
        {
            let item = &items[index];
            let label = truncate(&item.label, label_width);
            let padding = " ".repeat(label_width - label.chars().count());
            let room = columns.saturating_sub(label_width + 12);
            let detail = if item.detail.is_empty() {
                String::new()
            } else {
                format!(
                    "  {}{}{}",
                    style.dim,
                    truncate(&item.detail, room),
                    style.reset
                )
            };
            if position == state.selected {
                lines.push(format!(
                    "  {}{}▶  {label}{}{padding}{detail}",
                    style.bold, style.accent, style.reset
                ));
            } else {
                lines.push(format!("     {label}{padding}{detail}"));
            }
        }
        if visible.is_empty() {
            lines.push(format!("     {}no match{}", style.dim, style.reset));
        }
        if visible.len() > visible_rows {
            lines.push(String::new());
            lines.push(format!(
                "  {}{}/{}{}",
                style.dim,
                state.selected + 1,
                visible.len(),
                style.reset
            ));
        }
        terminal.draw(&lines);

        let key = terminal.read_key();
        match state.apply(key, items, visible_rows, screen.filterable) {
            Outcome::Chosen(index) => return Some(index),
            Outcome::Cancelled => return None,
            Outcome::Pending => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(labels: &[(&str, &str)]) -> Vec<Item> {
        labels.iter().map(|(l, d)| Item::new(*l, *d)).collect()
    }

    #[test]
    fn decodes_arrows_and_friends() {
        assert_eq!(parse_key(b"\x1b[A"), (Key::Up, 3));
        assert_eq!(parse_key(b"\x1b[B"), (Key::Down, 3));
        assert_eq!(parse_key(b"\x1b[C"), (Key::Right, 3));
        assert_eq!(parse_key(b"\x1b[D"), (Key::Left, 3));
        assert_eq!(parse_key(b"\x1bOA"), (Key::Up, 3)); // application cursor mode
        assert_eq!(parse_key(b"\x1b[5~"), (Key::PageUp, 4));
        assert_eq!(parse_key(b"\x1b[6~"), (Key::PageDown, 4));
        assert_eq!(parse_key(b"\x1b[H"), (Key::Home, 3));
        assert_eq!(parse_key(b"\x1b[4~"), (Key::End, 4));
        assert_eq!(parse_key(b"\x1b[3~"), (Key::Delete, 4));
        assert_eq!(parse_key(b"\x1b"), (Key::Esc, 1));
        assert_eq!(parse_key(b"\r"), (Key::Enter, 1));
        assert_eq!(parse_key(b"\x7f"), (Key::Backspace, 1));
        assert_eq!(parse_key(b"\x03"), (Key::CtrlC, 1));
        assert_eq!(parse_key(b"a"), (Key::Char('a'), 1));
        assert_eq!(parse_key("é".as_bytes()), (Key::Char('é'), 2));
        // only the first key is consumed
        assert_eq!(parse_key(b"\x1b[Bx"), (Key::Down, 3));
        assert_eq!(parse_key(b"xy"), (Key::Char('x'), 1));
    }

    #[test]
    fn plain_lists_move_with_arrows_and_vim_keys_and_choose_with_enter() {
        let list = items(&[("a", ""), ("b", ""), ("c", "")]);
        let mut state = ListState::default();
        assert_eq!(state.apply(Key::Down, &list, 5, false), Outcome::Pending);
        assert_eq!(
            state.apply(Key::Char('j'), &list, 5, false),
            Outcome::Pending
        );
        assert_eq!(state.apply(Key::Down, &list, 5, false), Outcome::Pending); // clamped at the end
        assert_eq!(state.selected, 2);
        assert_eq!(
            state.apply(Key::Char('k'), &list, 5, false),
            Outcome::Pending
        );
        assert_eq!(state.apply(Key::Up, &list, 5, false), Outcome::Pending);
        assert_eq!(state.apply(Key::Up, &list, 5, false), Outcome::Pending); // clamped at the start
        assert_eq!(state.selected, 0);
        state.apply(Key::End, &list, 5, false);
        assert_eq!(state.apply(Key::Enter, &list, 5, false), Outcome::Chosen(2));
        assert_eq!(state.apply(Key::Esc, &list, 5, false), Outcome::Cancelled);
        assert_eq!(
            state.apply(Key::Char('q'), &list, 5, false),
            Outcome::Cancelled
        );
        assert_eq!(state.apply(Key::CtrlC, &list, 5, false), Outcome::Cancelled);
    }

    #[test]
    fn scrolling_keeps_the_selection_in_view() {
        let list = items(&(0..30).map(|_| ("x", "")).collect::<Vec<_>>());
        let mut state = ListState::default();
        for _ in 0..12 {
            state.apply(Key::Down, &list, 5, false);
        }
        assert!(state.selected >= state.top && state.selected < state.top + 5);
        state.apply(Key::PageDown, &list, 5, false);
        assert_eq!(state.selected, 17);
        state.apply(Key::Home, &list, 5, false);
        assert_eq!((state.selected, state.top), (0, 0));
        state.apply(Key::End, &list, 5, false);
        assert_eq!(state.selected, 29);
        assert!(state.top + 5 > 29);
    }

    #[test]
    fn typing_filters_a_searchable_list_and_keeps_letters_as_text() {
        let list = items(&[
            ("us", "English (US)"),
            ("fr", "French"),
            ("de", "German"),
            ("be", "Belgian"),
        ]);
        let mut state = ListState::default();
        // j, k and q are text in a filterable list, not navigation or quit
        for c in "fr".chars() {
            assert_eq!(state.apply(Key::Char(c), &list, 5, true), Outcome::Pending);
        }
        assert_eq!(matching(&list, &state.filter), vec![1]);
        assert_eq!(state.apply(Key::Enter, &list, 5, true), Outcome::Chosen(1));
        state.apply(Key::Backspace, &list, 5, true);
        state.apply(Key::Backspace, &list, 5, true);
        assert!(state.filter.is_empty());
        // matches the description too, case-insensitively
        for c in "GERM".chars() {
            state.apply(Key::Char(c), &list, 5, true);
        }
        assert_eq!(matching(&list, &state.filter), vec![2]);
        // no match: Enter does nothing, Esc still cancels
        state.apply(Key::Char('z'), &list, 5, true);
        assert!(matching(&list, &state.filter).is_empty());
        assert_eq!(state.apply(Key::Enter, &list, 5, true), Outcome::Pending);
        assert_eq!(state.apply(Key::Esc, &list, 5, true), Outcome::Cancelled);
    }

    #[test]
    fn truncation_adds_an_ellipsis() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("a longer sentence", 8), "a longe…");
    }
}
