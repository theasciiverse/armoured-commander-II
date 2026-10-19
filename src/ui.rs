//! libtcod's input model and the terminal behind it. The original polls
//! `sys_check_for_event` in a loop everywhere it waits for the player and
//! calls `console_flush` to show the root console, capped at 50 frames a
//! second; `Screen` gives the same two calls over Ratatui and crossterm,
//! so those loops translate as they are.

use crate::color::Color;
use crate::console::Console;
use crate::glyph;
use ratatui::crossterm::event::{
    self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use ratatui::style::Color as TuiColor;
use std::time::{Duration, Instant};

/// `LIMIT_FPS = 50`.
const FRAME: Duration = Duration::from_millis(20);

/// libtcod key codes, the ones ArmCom tests for.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Vk {
    #[default]
    None,
    Enter,
    Escape,
    Backspace,
    Delete,
    Tab,
    Space,
    End,
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    /// `KEY_1` to `KEY_9`, `KEY_0`: the original takes 1 to 7 as well as
    /// F1 to F7.
    Digit(u8),
    F(u8),
    /// Any other printable key; the character is in `Key::c`.
    Char,
}

/// `libtcod.Key` after `sys_check_for_event`: `vk` is `None` when no key
/// was pressed this time round.
#[derive(Clone, Copy, Debug, Default)]
pub struct Key {
    pub vk: Vk,
    /// The character, `'\0'` for keys without one (`chr(key.c)`).
    pub c: char,
    pub shift: bool,
    pub ctrl: bool,
}

/// `libtcod.Mouse`: the cell under the pointer in root console
/// coordinates, and button presses from this event only.
#[derive(Clone, Copy, Debug, Default)]
pub struct Mouse {
    pub cx: i32,
    pub cy: i32,
    pub lbutton_pressed: bool,
    pub rbutton_pressed: bool,
    pub wheel_up: bool,
    pub wheel_down: bool,
}

pub struct Screen {
    term: Option<ratatui::DefaultTerminal>,
    last_flush: Instant,
    /// Where the root console's (0, 0) sat in the terminal at the last
    /// flush, to turn mouse positions into console cells.
    origin: (i32, i32),
    pub key: Key,
    pub mouse: Mouse,
    /// The terminal's new size, if it was resized since last asked.
    pub resized: Option<(i32, i32)>,
}

impl Screen {
    pub fn new(term: Option<ratatui::DefaultTerminal>) -> Self {
        Screen { term, last_flush: Instant::now(), origin: (0, 0), key: Key::default(), mouse: Mouse::default(), resized: None }
    }

    /// No terminal: tests and headless runs.
    pub fn headless() -> Self {
        Screen::new(None)
    }

    /// Hand the terminal back when the game is done with it.
    pub fn into_terminal(self) -> Option<ratatui::DefaultTerminal> {
        self.term
    }

    pub fn is_headless(&self) -> bool {
        self.term.is_none()
    }

    /// The terminal's size, or the root console's own when headless.
    pub fn size(&self) -> (i32, i32) {
        match &self.term {
            Some(t) => t.size().map(|s| (s.width as i32, s.height as i32)).unwrap_or((0, 0)),
            None => (0, 0),
        }
    }

    /// `console_flush`: show `root` centred in the terminal, then hold to
    /// the frame rate.
    pub fn flush(&mut self, root: &Console) {
        if let Some(term) = &mut self.term {
            let mut origin = self.origin;
            let _ = term.draw(|frame| {
                let area = frame.area();
                let buf = frame.buffer_mut();
                origin = present(root, area, buf);
            });
            self.origin = origin;
        }
        let elapsed = self.last_flush.elapsed();
        if elapsed < FRAME && !self.is_headless() {
            std::thread::sleep(FRAME - elapsed);
        }
        self.last_flush = Instant::now();
    }

    /// `sys_check_for_event(KEY_PRESS | MOUSE)`: take at most one pending
    /// event without waiting. Ctrl+C quits at once, as closing the
    /// original's window does (`sys.exit()`).
    pub fn check_for_event(&mut self) {
        self.key = Key::default();
        self.mouse.lbutton_pressed = false;
        self.mouse.rbutton_pressed = false;
        self.mouse.wheel_up = false;
        self.mouse.wheel_down = false;
        if self.term.is_none() {
            return;
        }
        if !event::poll(Duration::ZERO).unwrap_or(false) {
            return;
        }
        let Ok(ev) = event::read() else { return };
        match ev {
            Event::Key(k) if k.kind != KeyEventKind::Release => {
                log::debug!("input: {k:?}");
                if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) {
                    quit();
                }
                self.key = to_key(k.code, k.modifiers);
            }
            Event::Resize(w, h) => self.resized = Some((w as i32, h as i32)),
            Event::Mouse(m) => {
                self.mouse.cx = m.column as i32 - self.origin.0;
                self.mouse.cy = m.row as i32 - self.origin.1;
                match m.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        log::debug!("input: left click at {},{}", self.mouse.cx, self.mouse.cy);
                        self.mouse.lbutton_pressed = true;
                    }
                    MouseEventKind::Down(MouseButton::Right) => {
                        log::debug!("input: right click at {},{}", self.mouse.cx, self.mouse.cy);
                        self.mouse.rbutton_pressed = true;
                    }
                    MouseEventKind::ScrollUp => self.mouse.wheel_up = true,
                    MouseEventKind::ScrollDown => self.mouse.wheel_down = true,
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

/// Leave the terminal as it was and stop, from anywhere.
pub fn quit() -> ! {
    let _ = ratatui::crossterm::execute!(std::io::stdout(), event::DisableMouseCapture);
    ratatui::restore();
    log::info!("quit");
    std::process::exit(0);
}

fn to_key(code: KeyCode, mods: KeyModifiers) -> Key {
    let shift = mods.contains(KeyModifiers::SHIFT);
    let ctrl = mods.contains(KeyModifiers::CONTROL);
    let (vk, c) = match code {
        KeyCode::Enter => (Vk::Enter, '\r'),
        KeyCode::Esc => (Vk::Escape, '\0'),
        KeyCode::Backspace => (Vk::Backspace, '\0'),
        KeyCode::Delete => (Vk::Delete, '\0'),
        KeyCode::Tab => (Vk::Tab, '\t'),
        KeyCode::BackTab => return Key { vk: Vk::Tab, c: '\t', shift: true, ctrl },
        KeyCode::End => (Vk::End, '\0'),
        KeyCode::Home => (Vk::Home, '\0'),
        KeyCode::Up => (Vk::Up, '\0'),
        KeyCode::Down => (Vk::Down, '\0'),
        KeyCode::Left => (Vk::Left, '\0'),
        KeyCode::Right => (Vk::Right, '\0'),
        KeyCode::PageUp => (Vk::PageUp, '\0'),
        KeyCode::PageDown => (Vk::PageDown, '\0'),
        KeyCode::F(n) => (Vk::F(n), '\0'),
        KeyCode::Char(' ') => (Vk::Space, ' '),
        KeyCode::Char(ch) if ch.is_ascii_digit() => (Vk::Digit(ch as u8 - b'0'), ch),
        KeyCode::Char(ch) => (Vk::Char, ch),
        _ => (Vk::None, '\0'),
    };
    Key { vk, c, shift: shift || c.is_ascii_uppercase(), ctrl }
}

/// Draw a console centred in the terminal area; returns where its (0, 0)
/// landed. A terminal smaller than the console shows its middle.
pub fn present(con: &Console, area: ratatui::layout::Rect, buf: &mut ratatui::buffer::Buffer) -> (i32, i32) {
    let (aw, ah) = (area.width as i32, area.height as i32);
    let ox = (aw - con.w) / 2;
    let oy = (ah - con.h) / 2;
    for sy in 0..ah {
        for sx in 0..aw {
            let cell = &mut buf[(area.x + sx as u16, area.y + sy as u16)];
            match con.cell(sx - ox, sy - oy) {
                Some(c) => {
                    cell.set_char(glyph::to_char(c.ch)).set_fg(rgb(c.fg)).set_bg(rgb(c.bg));
                }
                None => {
                    cell.set_char(' ').set_fg(TuiColor::Reset).set_bg(TuiColor::Black);
                }
            }
        }
    }
    if con.w > aw || con.h > ah {
        let text = format!("Terminal is {aw}x{ah}; Armoured Commander needs at least {}x{}", con.w, con.h);
        let y = area.y + area.height.saturating_sub(1);
        for (i, ch) in text.chars().take(aw as usize).enumerate() {
            buf[(area.x + i as u16, y)].set_char(ch).set_fg(TuiColor::Black).set_bg(TuiColor::Rgb(0, 200, 255));
        }
    }
    (area.x as i32 + ox, area.y as i32 + oy)
}

pub fn rgb(c: Color) -> TuiColor {
    TuiColor::Rgb(c.0, c.1, c.2)
}

/// Python's `textwrap.wrap(text, width, subsequent_indent=indent)`:
/// whitespace runs become single breaks between words, words longer than
/// a line are split, and an empty text gives no lines.
pub fn wrap(text: &str, width: usize, indent: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut line_len = 0usize;
    for word in text.split_whitespace() {
        let mut word: Vec<char> = word.chars().collect();
        loop {
            let prefix = if lines.is_empty() { 0 } else { indent.chars().count() };
            let room = width.saturating_sub(prefix).max(1);
            let need = if line_len == 0 { word.len() } else { line_len + 1 + word.len() };
            if need <= room {
                if line_len > 0 {
                    line.push(' ');
                    line_len += 1;
                }
                line.extend(word.iter());
                line_len += word.len();
                break;
            }
            if line_len > 0 {
                push_line(&mut lines, &mut line, &mut line_len, indent);
                continue;
            }
            // a word longer than a whole line is broken
            let rest = word.split_off(room);
            line.extend(word.iter());
            line_len = room;
            push_line(&mut lines, &mut line, &mut line_len, indent);
            word = rest;
            if word.is_empty() {
                break;
            }
        }
    }
    if line_len > 0 {
        push_line(&mut lines, &mut line, &mut line_len, indent);
    }
    lines
}

fn push_line(lines: &mut Vec<String>, line: &mut String, line_len: &mut usize, indent: &str) {
    let text = std::mem::take(line);
    lines.push(if lines.is_empty() { text } else { format!("{indent}{text}") });
    *line_len = 0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_like_textwrap() {
        assert_eq!(wrap("one two three", 7, ""), vec!["one two", "three"]);
        assert_eq!(wrap("one two three", 8, " "), vec!["one two", " three"]);
        assert_eq!(wrap("", 10, ""), Vec::<String>::new());
        assert_eq!(wrap("abcdefghij", 4, ""), vec!["abcd", "efgh", "ij"]);
        assert_eq!(wrap("a\nb  c", 10, ""), vec!["a b c"]);
    }
}
