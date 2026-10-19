//! The main menu, from the end of armcom2.py: the title picture
//! (`main_title.xp`, 90x60 square cells) with a random tank's portrait and
//! the poppy on it, a light sweeping across the title letters, and the
//! menu; before it, the studio logo and the disclaimer.
//!
//! The picture is drawn as art (see `art.rs`), as big as the screen
//! allows in its own proportions: 90x30 at 100x30. Its text (the menu,
//! version and copyright) is drawn as terminal text at the rows the
//! original put it on, scaled.

use crate::art;
use crate::color::{self, Color};
use crate::console::{Align, Console};
use crate::layout::*;
use crate::rng::Rng;
use crate::ui::wrap;
use std::time::{Duration, Instant};

pub const VERSION: &str = "3.0.2";
const DISCLAIMER: &str =
    "This is a work of fiction and no endorsement of any historical ideologies or events depicted within is intended.";

/// `ACTION_KEY_COL`
pub const ACTION_KEY_COL: Color = Color(51, 153, 255);

/// The light sweeping across the title, ten columns wide.
const GRADIENT: [Color; 10] = [
    Color(51, 51, 51),
    Color(64, 64, 64),
    Color(128, 128, 128),
    Color(192, 192, 192),
    Color(255, 255, 255),
    Color(192, 192, 192),
    Color(128, 128, 128),
    Color(64, 64, 64),
    Color(51, 51, 51),
    Color(51, 51, 51),
];
const GRADIENT_EVERY: Duration = Duration::from_millis(60);

pub enum Choice {
    Continue,
    Load,
    NewCampaign,
    Options,
    Quit,
}

pub struct MainMenu {
    pub con: Console,
    /// The title picture with the portrait and poppy on it, 90x60.
    title: Console,
    gradient_x: i32,
    click: Instant,
    /// Whether there is a saved campaign to continue (none yet: saves
    /// are not ported).
    saved: bool,
    /// A line under the menu, for choices not ported yet.
    pub note: Option<String>,
}

impl MainMenu {
    pub fn new(rng: &mut Rng) -> Self {
        let mut title = crate::xp::by_name("main_title.xp").unwrap_or_else(|| Console::new(WINDOW_WIDTH, WINDOW_HEIGHT));
        if let Some(portrait) = random_tank_portrait(rng) {
            blit_art(&portrait, &mut title, 7, 6);
        }
        if let Some(poppy) = crate::xp::by_name("poppy.xp") {
            blit_art(&poppy, &mut title, 1, WINDOW_HEIGHT - 8);
        }
        let mut menu = MainMenu {
            con: Console::new(sw(), sh()),
            title,
            gradient_x: WINDOW_WIDTH + 5,
            click: Instant::now(),
            saved: false,
            note: None,
        };
        menu.draw();
        menu
    }

    /// Where the title picture goes on the screen: left, top, columns, rows.
    fn placement(&self) -> (i32, i32, i32, i32) {
        let (cols, rows) = art::fit(&self.title, sw(), sh());
        ((sw() - cols) / 2, (sh() - rows) / 2, cols, rows)
    }

    /// The screen row for a row of the original's 60.
    fn row(&self, y: i32) -> i32 {
        let (_, top, _, rows) = self.placement();
        top + y * rows / WINDOW_HEIGHT
    }

    /// `UpdateMainTitleCon`
    pub fn draw(&mut self) {
        let (left, top, cols, rows) = self.placement();
        let c = &mut self.con;
        c.bg = color::BLACK;
        c.clear();
        art::draw(&self.title, c, left, top, cols, rows);

        let xm = sxm();
        let mut y = self.row(38);
        for (key, text) in [('C', "Continue"), ('L', "Load Campaign"), ('N', "New Campaign"), ('O', "Options"), ('Q', "Quit")] {
            let disabled = (key == 'C' || key == 'L') && !self.saved;
            let (kc, tc) = if disabled { (color::DARK_GREY, color::DARK_GREY) } else { (ACTION_KEY_COL, color::LIGHTER_GREY) };
            let c = &mut self.con;
            c.fg = kc;
            c.print_ex(xm - 6, y, Align::Left, &key.to_string());
            c.fg = tc;
            c.print_ex(xm - 4, y, Align::Left, text);
            y += 1;
        }
        let (n1, n2, n3) = (self.row(55), self.row(57), self.row(58));
        let c = &mut self.con;
        c.fg = color::LIGHT_GREY;
        c.print_ex(xm, n1, Align::Center, VERSION);
        c.print_ex(xm, n2, Align::Center, "Copyright 2016-2020 Gregory Adam Scott");
        c.print_ex(xm, n3, Align::Center, "Open Source under the GNU GPL");
        if let Some(note) = &self.note {
            c.fg = color::LIGHT_RED;
            c.print_ex(xm, y + 1, Align::Center, note);
        }
        c.fg = color::WHITE;
    }

    /// `AnimateMainMenu`: move the light two columns left; true when the
    /// screen changed.
    pub fn tick(&mut self) -> bool {
        if self.click.elapsed() < GRADIENT_EVERY {
            return false;
        }
        self.click = Instant::now();
        for (x, col) in GRADIENT.iter().enumerate() {
            let gx = x as i32 + self.gradient_x;
            if gx > WINDOW_WIDTH {
                continue;
            }
            for y in 19..34 {
                if let Some(mut cell) = self.title.cell(gx, y) {
                    if cell.ch != 0 && cell.fg != *col {
                        cell.fg = *col;
                        self.title.set_cell(gx, y, cell);
                    }
                }
            }
        }
        self.gradient_x -= 2;
        if self.gradient_x <= 0 {
            self.gradient_x = WINDOW_WIDTH + 10;
        }
        self.draw();
        true
    }

    pub fn key(&mut self, c: char) -> Option<Choice> {
        let choice = match c.to_ascii_lowercase() {
            'c' if self.saved => Choice::Continue,
            'l' if self.saved => Choice::Load,
            'n' => Choice::NewCampaign,
            'o' => Choice::Options,
            'q' => Choice::Quit,
            _ => return None,
        };
        Some(choice)
    }
}

/// `console_blit` of art onto art, skipping transparent cells.
fn blit_art(src: &Console, dst: &mut Console, x: i32, y: i32) {
    let mut src = src.clone();
    src.key = Some(art::KEY);
    Console::blit(&src, 0, 0, src.w, src.h, dst, x, y);
}

/// `Session`'s `tank_portrait`: a random tank's portrait from the unit
/// definitions.
fn random_tank_portrait(rng: &mut Rng) -> Option<Console> {
    let defs: serde_json::Value = serde_json::from_str(include_str!("../data/unit_type_defs.json")).ok()?;
    let units = defs.as_object()?;
    let tanks: Vec<&str> = units
        .values()
        .filter(|u| matches!(u["class"].as_str(), Some("Light Tank" | "Medium Tank" | "Heavy Tank" | "Tank Destroyer")))
        .filter_map(|u| u["portrait"].as_str())
        .filter(|p| crate::xp::FILES.iter().any(|(n, _)| n == p))
        .collect();
    if tanks.is_empty() {
        return None;
    }
    let name: &str = *rng.choice(&tanks);
    crate::xp::by_name(name)
}

/// The studio logo and the disclaimer, shown for two seconds before the
/// menu (`Wait(200)` is in hundredths).
pub fn splash() -> Console {
    let mut c = Console::new(sw(), sh());
    let xm = sxm();
    let lines = wrap(DISCLAIMER, 40, "");
    let text_rows = 3 + lines.len() as i32;
    if let Some(cats) = crate::xp::by_name("cats.xp") {
        let (cols, rows) = art::fit(&cats, sw(), sh() - text_rows - 1);
        let top = (sh() - rows - text_rows) / 2;
        art::draw(&cats, &mut c, xm - cols / 2, top, cols, rows);
        let mut y = top + rows + 1;
        c.fg = color::WHITE;
        c.print_ex(xm, y, Align::Center, "Copyright 2016-2020");
        c.fg = color::LIGHT_GREY;
        y += 2;
        for line in &lines {
            c.print_ex(xm, y, Align::Center, line);
            y += 1;
        }
    }
    c
}

pub const SPLASH_FOR: Duration = Duration::from_secs(2);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_fits_the_smallest_screen() {
        let mut rng = Rng::new(3);
        let m = MainMenu::new(&mut rng);
        let (left, top, cols, rows) = m.placement();
        assert!(left >= 0 && top >= 0 && cols <= MIN_WIDTH && rows <= MIN_HEIGHT);
        assert!(m.row(58) < MIN_HEIGHT && m.row(38) + 5 < m.row(55));
    }

    #[test]
    fn there_are_tank_portraits() {
        let mut rng = Rng::new(9);
        assert!(random_tank_portrait(&mut rng).is_some());
    }
}
