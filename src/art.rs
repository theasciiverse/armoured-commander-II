//! II's art in the terminal. The original's cells are square (8x8 pixels)
//! and its art is pixel art: solid blocks, half and quarter blocks and
//! corner triangles, each cell a 2x2 grid of pixels in effect. A terminal
//! cell is about twice as tall as wide, so a half block (`▀`, its top in
//! the foreground colour, its bottom in the background) is a pair of
//! square pixels: one original cell is two terminal columns of one row.
//!
//! `draw` puts art on a console at any size: its pixels (two per cell each
//! way, from each glyph's measured pattern, `glyph::QUADS`) are resampled
//! to the cells given, two pixels to a cell, nearest pixel. With a column
//! or more for each of the art's cells, glyphs that are drawings rather
//! than pixels (letters, a star, the poppy) are put in as themselves, one
//! character to a column in runs, as text reads in a terminal; smaller,
//! they are pixels like the rest.

use crate::color::Color;
use crate::console::{Cell, Console};
use crate::glyph::{self, QUADS};

/// The background REXPaint uses for "transparent".
pub const KEY: Color = Color(255, 0, 255);

/// A cell's 2x2 pixels, top left, top right, bottom left, bottom right:
/// the foreground where its glyph's pattern is set, else the background;
/// `None` where the cell is transparent.
fn pixels(c: Cell) -> [Option<Color>; 4] {
    let bits = QUADS.get(c.ch as usize).copied().unwrap_or(0);
    let mut px = [None; 4];
    for (n, p) in px.iter_mut().enumerate() {
        let col = if bits & (1 << n) != 0 { c.fg } else { c.bg };
        *p = (col != KEY).then_some(col);
    }
    px
}

/// The art's own size in terminal cells: two columns a cell, one row.
pub fn native_size(src: &Console) -> (i32, i32) {
    (src.w * 2, src.h)
}

/// The biggest size that keeps the art's proportions within `max_w` by
/// `max_h` terminal cells.
pub fn fit(src: &Console, max_w: i32, max_h: i32) -> (i32, i32) {
    // the art is src.w by src.h square cells; a terminal cell is half as
    // wide as tall, so the art takes twice its width in columns
    let by_h = (max_h, (2 * src.w * max_h + src.h / 2) / src.h.max(1));
    if by_h.1 <= max_w {
        (by_h.1, by_h.0)
    } else {
        (max_w, (max_w * src.h + src.w) / (2 * src.w).max(1))
    }
}

/// Draw `src` at (x, y) on `dst`, `cols` by `rows` cells.
pub fn draw(src: &Console, dst: &mut Console, x: i32, y: i32, cols: i32, rows: i32) {
    draw_pixels(src, dst, x, y, cols, rows);
    // with a column or more for each cell, drawings (letters, symbols) go in as
    // themselves: each run of them (over one-cell gaps between words) one
    // character to a column, from where its first falls, as text and
    // ASCII art read in a terminal
    if cols >= src.w && rows * 2 >= src.h && (cols, rows) != native_size(src) {
        let drawing = |c: Cell| !glyph::is_pixels(c.ch);
        for sy in 0..src.h {
            let ty = y + sy * rows / src.h;
            let mut sx = 0;
            while sx < src.w {
                let cell = src.cell(sx, sy).expect("inside the art");
                if !drawing(cell) {
                    sx += 1;
                    continue;
                }
                let mut tx = x + sx * cols / src.w;
                while let Some(c) = src.cell(sx, sy) {
                    let next_is_text = src.cell(sx + 1, sy).is_some_and(drawing);
                    if !drawing(c) && !next_is_text {
                        break;
                    }
                    let bg = if c.bg == KEY { dst.char_bg(tx, ty) } else { c.bg };
                    let ch = if drawing(c) { c.ch } else { 32 };
                    dst.put_char_ex(tx, ty, ch, c.fg, bg);
                    tx += 1;
                    sx += 1;
                }
            }
        }
    }
}

fn draw_pixels(src: &Console, dst: &mut Console, x: i32, y: i32, cols: i32, rows: i32) {
    let native = (cols, rows) == native_size(src);
    let (px_w, px_h) = (src.w * 2, src.h * 2);
    let pixel = |px: i32, py: i32| -> Option<Color> {
        let sx = px * px_w / cols.max(1);
        let sy = py * px_h / (rows * 2).max(1);
        let cell = src.cell(sx / 2, sy / 2)?;
        pixels(cell)[((sy % 2) * 2 + sx % 2) as usize]
    };
    for ty in 0..rows {
        for tx in 0..cols {
            let (dx, dy) = (x + tx, y + ty);
            if native {
                let cell = src.cell(tx / 2, ty).expect("inside the art");
                if !glyph::is_pixels(cell.ch) {
                    if cell.bg == KEY {
                        continue;
                    }
                    let ch = if tx % 2 == 0 { cell.ch } else { 32 };
                    dst.put_char_ex(dx, dy, ch, cell.fg, cell.bg);
                    continue;
                }
            }
            let top = pixel(tx, ty * 2);
            let bottom = pixel(tx, ty * 2 + 1);
            let under = dst.cell(dx, dy).map(|c| c.bg).unwrap_or_default();
            match (top, bottom) {
                (None, None) => {}
                (t, b) => dst.put_char_ex(dx, dy, 223, t.unwrap_or(under), b.unwrap_or(under)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color;

    #[test]
    fn a_quarter_block_is_one_pixel_of_four() {
        let mut src = Console::new(1, 1);
        src.put_char_ex(0, 0, 224, color::RED, color::BLUE);
        let mut dst = Console::new(2, 1);
        draw(&src, &mut dst, 0, 0, 2, 1);
        let (l, r) = (dst.cell(0, 0).unwrap(), dst.cell(1, 0).unwrap());
        assert_eq!((l.ch, l.fg, l.bg), (223, color::RED, color::BLUE));
        assert_eq!((r.fg, r.bg), (color::BLUE, color::BLUE));
    }

    #[test]
    fn transparent_cells_leave_what_is_under_them() {
        let mut src = Console::new(1, 1);
        src.put_char_ex(0, 0, 0, color::WHITE, KEY);
        let mut dst = Console::new(2, 1);
        dst.put_char_ex(0, 0, 65, color::WHITE, color::GREEN);
        draw(&src, &mut dst, 0, 0, 2, 1);
        assert_eq!(dst.cell(0, 0).unwrap().ch, 65);
    }

    #[test]
    fn letters_stay_letters_at_their_own_size() {
        let mut src = Console::new(1, 1);
        src.put_char_ex(0, 0, 65, color::WHITE, color::BLACK);
        let mut dst = Console::new(2, 1);
        draw(&src, &mut dst, 0, 0, 2, 1);
        assert_eq!(dst.cell(0, 0).unwrap().ch, 65);
    }

    #[test]
    fn fitting_keeps_the_proportions() {
        let title = Console::new(90, 60);
        assert_eq!(fit(&title, 100, 30), (90, 30));
        assert_eq!(fit(&title, 60, 30), (60, 20));
    }
}
