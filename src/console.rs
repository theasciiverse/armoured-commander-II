//! A libtcod console: a grid of glyph codes (0 to 287 in II's font) with foreground and background
//! colours, plus the handful of operations ArmCom draws with. Keeping the
//! same model as the original means its drawing code translates line for
//! line; only the final step, `present`, knows about the terminal.

use crate::color::{self, Color};
use crate::glyph;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    pub ch: u16,
    pub fg: Color,
    pub bg: Color,
}

#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// libtcod's colour controls: codes 1 to 5 in printed text switch to one
/// of five colour pairs, 8 switches back.
pub const COLCTRL_1: u16 = 1;
pub const COLCTRL_STOP: u16 = 8;

#[derive(Clone)]
pub struct Console {
    pub w: i32,
    pub h: i32,
    cells: Vec<Cell>,
    pub fg: Color,
    pub bg: Color,
    pub align: Align,
    /// Cells with this background are skipped when the console is blitted.
    pub key: Option<Color>,
    controls: [(Color, Color); 5],
}

impl Console {
    pub fn new(w: i32, h: i32) -> Self {
        let blank = Cell { ch: 32, fg: color::WHITE, bg: color::BLACK };
        Console {
            w,
            h,
            cells: vec![blank; (w * h) as usize],
            fg: color::WHITE,
            bg: color::BLACK,
            align: Align::Left,
            key: None,
            controls: [(color::WHITE, color::BLACK); 5],
        }
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        (x >= 0 && y >= 0 && x < self.w && y < self.h).then(|| (y * self.w + x) as usize)
    }

    pub fn cell(&self, x: i32, y: i32) -> Option<Cell> {
        self.index(x, y).map(|i| self.cells[i])
    }

    pub fn set_cell(&mut self, x: i32, y: i32, cell: Cell) {
        if let Some(i) = self.index(x, y) {
            self.cells[i] = cell;
        }
    }

    pub fn set_color_control(&mut self, n: u16, fg: Color, bg: Color) {
        self.controls[(n - 1) as usize] = (fg, bg);
    }

    /// `console_clear`: spaces in the default colours.
    pub fn clear(&mut self) {
        let blank = Cell { ch: 32, fg: self.fg, bg: self.bg };
        self.cells.fill(blank);
    }

    /// `console_rect` with `BKGND_SET`: the default background over an
    /// area, and with `clear` also spaces.
    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, clear: bool) {
        for yy in y..y + h {
            for xx in x..x + w {
                if let Some(i) = self.index(xx, yy) {
                    self.cells[i].bg = self.bg;
                    if clear {
                        self.cells[i].ch = 32;
                    }
                }
            }
        }
    }

    /// `console_put_char_ex`.
    #[allow(dead_code)]
    pub fn put_char_ex(&mut self, x: i32, y: i32, ch: u16, fg: Color, bg: Color) {
        self.set_cell(x, y, Cell { ch, fg, bg });
    }

    #[allow(dead_code)]
    /// `console_put_char` with `BKGND_NONE`: the glyph in the default
    /// foreground, background untouched.
    pub fn put_char(&mut self, x: i32, y: i32, ch: u16) {
        if let Some(i) = self.index(x, y) {
            self.cells[i].ch = ch;
            self.cells[i].fg = self.fg;
        }
    }

    /// `console_print` with the console's alignment, background untouched.
    /// Colour controls in the text change the foreground and are not drawn.
    pub fn print(&mut self, x: i32, y: i32, text: &str) {
        self.print_ex(x, y, self.align, text);
    }

    /// `console_print_ex` with `BKGND_NONE`.
    pub fn print_ex(&mut self, x: i32, y: i32, align: Align, text: &str) {
        let codes = glyph::encode(text);
        let width = codes.iter().filter(|&&c| !is_control(c)).count() as i32;
        let mut cx = match align {
            Align::Left => x,
            Align::Center => x - width / 2,
            Align::Right => x - width + 1,
        };
        let mut fg = self.fg;
        for c in codes {
            match c {
                1..=5 => fg = self.controls[(c - 1) as usize].0,
                COLCTRL_STOP => fg = self.fg,
                _ => {
                    if let Some(i) = self.index(cx, y) {
                        self.cells[i].ch = c;
                        self.cells[i].fg = fg;
                    }
                    cx += 1;
                }
            }
        }
    }

    /// `console_blit` at full opacity: copies glyphs and both colours,
    /// skipping cells whose background is the source's key colour.
    /// Blitting a console onto itself works, as the title animation does.
    #[allow(clippy::too_many_arguments)]
    pub fn blit(src: &Console, sx: i32, sy: i32, w: i32, h: i32, dst: &mut Console, dx: i32, dy: i32) {
        for y in 0..h {
            for x in 0..w {
                let Some(cell) = src.cell(sx + x, sy + y) else { continue };
                if src.key == Some(cell.bg) {
                    continue;
                }
                dst.set_cell(dx + x, dy + y, cell);
            }
        }
    }

    /// `console_blit` at foreground 1.0, background 0.0: glyphs and their
    /// colour only, over whatever background is already there, skipping
    /// cells whose background is the source's key colour.
    #[allow(clippy::too_many_arguments)]
    pub fn blit_fg(src: &Console, sx: i32, sy: i32, w: i32, h: i32, dst: &mut Console, dx: i32, dy: i32) {
        for y in 0..h {
            for x in 0..w {
                let Some(cell) = src.cell(sx + x, sy + y) else { continue };
                if src.key == Some(cell.bg) {
                    continue;
                }
                if let Some(i) = dst.index(dx + x, dy + y) {
                    dst.cells[i].ch = cell.ch;
                    dst.cells[i].fg = cell.fg;
                }
            }
        }
    }

    /// `console_get_char`; 0 outside the console.
    pub fn char_at(&self, x: i32, y: i32) -> u16 {
        self.cell(x, y).map_or(0, |c| c.ch)
    }

    /// Blit within one console (`console_blit(c, ..., c, ...)`).
    pub fn blit_self(&mut self, sx: i32, sy: i32, w: i32, h: i32, dx: i32, dy: i32) {
        let src = self.clone();
        Console::blit(&src, sx, sy, w, h, self, dx, dy);
    }

    /// The original darkens the screen behind every window by blitting a
    /// cleared black console over it at foreground 0.0 and background 0.7
    /// (`console_blit(con, ..., 0, 0, 0, 0.0, 0.7)`). For a blank black
    /// source, libtcod's faded blit moves both colours of each cell 70% of
    /// the way to black and keeps the glyphs.
    pub fn darken(&mut self, amount: f32) {
        for c in &mut self.cells {
            c.fg = color::lerp(c.fg, color::BLACK, amount);
            c.bg = color::lerp(c.bg, color::BLACK, amount);
        }
    }

    /// `console_print_ex` with `BKGND_SET`: as `print_ex`, and the default
    /// background under every printed cell.
    pub fn print_ex_bg(&mut self, x: i32, y: i32, align: Align, text: &str) {
        let width = glyph::encode(text).iter().filter(|&&c| !is_control(c)).count() as i32;
        let x0 = match align {
            Align::Left => x,
            Align::Center => x - width / 2,
            Align::Right => x - width + 1,
        };
        for i in 0..width {
            if let Some(n) = self.index(x0 + i, y) {
                self.cells[n].bg = self.bg;
            }
        }
        self.print_ex(x, y, align, text);
    }

    /// `console_put_char` with `BKGND_SET`.
    pub fn put_char_bg(&mut self, x: i32, y: i32, ch: u16) {
        let (fg, bg) = (self.fg, self.bg);
        self.set_cell(x, y, Cell { ch, fg, bg });
    }

    /// `console_set_char_background`.
    pub fn set_char_bg(&mut self, x: i32, y: i32, bg: Color) {
        if let Some(i) = self.index(x, y) {
            self.cells[i].bg = bg;
        }
    }

    /// `console_set_char_foreground`.
    pub fn set_char_fg(&mut self, x: i32, y: i32, fg: Color) {
        if let Some(i) = self.index(x, y) {
            self.cells[i].fg = fg;
        }
    }

    /// `console_get_char_background`; black outside the console, as libtcod.
    pub fn char_bg(&self, x: i32, y: i32) -> Color {
        self.cell(x, y).map_or(color::BLACK, |c| c.bg)
    }

    /// `console_hline` with `BKGND_DEFAULT`: a run of single lines in the
    /// default foreground.
    pub fn hline(&mut self, x: i32, y: i32, len: i32) {
        for i in 0..len {
            self.put_char(x + i, y, 196);
        }
    }

    /// `console_vline` with `BKGND_DEFAULT`.
    pub fn vline(&mut self, x: i32, y: i32, len: i32) {
        for i in 0..len {
            self.put_char(x, y + i, 179);
        }
    }

    /// `console_print_frame`: a single-line box in the default foreground.
    /// `set_bg` is `BKGND_SET` for the border (otherwise the background is
    /// left alone, as `BKGND_DEFAULT` does); `clear` fills the inside with
    /// the default background.
    pub fn frame(&mut self, x: i32, y: i32, w: i32, h: i32, clear: bool, set_bg: bool) {
        if clear {
            self.rect(x + 1, y + 1, w - 2, h - 2, true);
        }
        let put = |con: &mut Console, xx, yy, ch| {
            if set_bg { con.put_char_bg(xx, yy, ch) } else { con.put_char(xx, yy, ch) }
        };
        for i in 1..w - 1 {
            put(self, x + i, y, 196);
            put(self, x + i, y + h - 1, 196);
        }
        for i in 1..h - 1 {
            put(self, x, y + i, 179);
            put(self, x + w - 1, y + i, 179);
        }
        put(self, x, y, 218);
        put(self, x + w - 1, y, 191);
        put(self, x, y + h - 1, 192);
        put(self, x + w - 1, y + h - 1, 217);
    }
}

fn is_control(c: u16) -> bool {
    matches!(c, 1..=5 | COLCTRL_STOP)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centred_print_skips_colour_controls() {
        let mut con = Console::new(20, 1);
        con.set_color_control(COLCTRL_1, color::BLUE, color::BLACK);
        con.print_ex(10, 0, Align::Center, "[\u{1}Q\u{8}]uit");
        let row: String = (0..20).map(|x| glyph::to_char(con.cell(x, 0).unwrap().ch)).collect();
        assert_eq!(row, "       [Q]uit       ");
        assert_eq!(con.cell(8, 0).unwrap().fg, color::BLUE);
        assert_eq!(con.cell(9, 0).unwrap().fg, color::WHITE);
    }

    #[test]
    fn key_colour_is_transparent() {
        let mut src = Console::new(2, 1);
        src.key = Some(Color(255, 0, 255));
        src.put_char_ex(0, 0, 65, color::WHITE, Color(255, 0, 255));
        src.put_char_ex(1, 0, 66, color::WHITE, color::BLACK);
        let mut dst = Console::new(2, 1);
        dst.put_char_ex(0, 0, 120, color::RED, color::RED);
        Console::blit(&src, 0, 0, 2, 1, &mut dst, 0, 0);
        assert_eq!(dst.cell(0, 0).unwrap().ch, 120);
        assert_eq!(dst.cell(1, 0).unwrap().ch, 66);
    }
}
