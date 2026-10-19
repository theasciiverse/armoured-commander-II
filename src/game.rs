//! The original's globals (`session`, `campaign`, `campaign_day`,
//! `scenario`, `config`, the root console, `key`) as one `Game`, and its
//! general functions: waiting for keys, messages, notifications, text
//! input, the game menu and options, keyboard layouts.
//!
//! Windows and menus are drawn on a 100x30 page (`layout::PAGE_W`,
//! `PAGE_H`) centred on the screen; the maps use the whole screen.

use crate::campaign::Campaign;
use crate::cday::CampaignDay;
use crate::color::{self, Color};
use crate::console::{Align, Console};
use crate::defs::{self, engine, text};
use crate::layout::*;
use crate::rng::Rng;
use crate::ui::{Key, Screen, Vk, wrap};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

pub const NAME: &str = "Armoured Commander II";
pub const VERSION: &str = "3.0.2";

pub const ACTION_KEY_COL: Color = Color(51, 153, 255);
pub const HIGHLIGHT_MENU_COL: Color = Color(30, 70, 130);
pub const TITLE_COL: Color = Color(102, 178, 255);
pub const PORTRAIT_BG_COL: Color = Color(217, 108, 0);
pub const GOLD_COL: Color = Color(255, 255, 100);
pub const KEY_COLOR: Color = Color(255, 0, 255);

pub const MAX_TANK_NAME_LENGTH: usize = 20;
pub const MAX_NICKNAME_LENGTH: usize = 10;

pub const KEYBOARDS: [&str; 5] = ["QWERTY", "AZERTY", "QWERTZ", "Dvorak", "Custom"];

/// `armcom2.cfg`. The font size option is gone: the terminal's font is
/// the terminal's.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub sounds_enabled: bool,
    pub master_volume: i32,
    pub animation_speed: i32,
    /// 0 short, 1 normal, 2 long.
    pub message_pause: i32,
    /// An index into `KEYBOARDS`.
    pub keyboard: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { sounds_enabled: true, master_volume: 10, animation_speed: 1, message_pause: 1, keyboard: 0 }
    }
}

/// The player's data folder: settings and saved campaigns.
pub fn data_dir() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(base.join("armoured-commander-ii"))
}

impl Settings {
    /// `LoadCFG`
    pub fn load() -> Self {
        let Some(path) = data_dir().map(|d| d.join("settings.json")) else { return Settings::default() };
        match std::fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_else(|e| {
                log::warn!("settings: {e}");
                Settings::default()
            }),
            Err(_) => {
                let s = Settings::default();
                s.save();
                s
            }
        }
    }

    /// `SaveCFG`
    pub fn save(&self) {
        let Some(dir) = data_dir() else { return };
        let _ = std::fs::create_dir_all(&dir);
        if let Err(e) = std::fs::write(dir.join("settings.json"), serde_json::to_string_pretty(self).unwrap()) {
            log::warn!("settings: {e}");
        }
    }
}

impl Settings {
    /// `ChangeGameSettings`: true if the setting was changed (and saved).
    pub fn change(&mut self, key_char: char) -> bool {
        match key_char {
            's' => self.sounds_enabled = !self.sounds_enabled,
            'v' => self.master_volume = if self.master_volume >= 10 { 1 } else { self.master_volume + 1 },
            'p' => self.message_pause = if self.message_pause >= 2 { 0 } else { self.message_pause + 1 },
            'k' => self.keyboard = (self.keyboard + 1) % KEYBOARDS.len(),
            _ => return false,
        }
        log::info!("settings: {self:?}");
        crate::sound::configure(self.sounds_enabled, self.master_volume);
        self.save();
        true
    }
}

/// `DisplayGameOptions`: the options and their settings, from (x, y).
pub fn draw_game_options(settings: &Settings, c: &mut Console, x: i32, mut y: i32, skip_esc: bool) {
    for (ch, label) in
        [("S", "Sound Effects"), ("V", "Master Volume"), ("P", "Message Pause"), ("K", "Keyboard"), ("Esc", "Return to Main Menu")]
    {
        if ch == "Esc" {
            if skip_esc {
                continue;
            }
            y += 1;
        }
        key_line(c, x, x + 4, y, ch, label, color::LIGHTER_GREY);
        c.fg = color::LIGHT_BLUE;
        let value = match ch {
            "S" => if settings.sounds_enabled { "ON".to_string() } else { "OFF".to_string() },
            "V" => settings.master_volume.to_string(),
            "P" => ["Short", "Normal", "Long"][settings.message_pause.clamp(0, 2) as usize].to_string(),
            "K" => KEYBOARDS[settings.keyboard.min(KEYBOARDS.len() - 1)].to_string(),
            _ => String::new(),
        };
        c.print_ex(x + 20, y, Align::Left, &value);
        y += 1;
    }
}

/// `GetDateText`: "1939.09.01" as "September 1, 1939".
pub fn date_text(date: &str) -> String {
    let parts: Vec<&str> = date.split('.').collect();
    if parts.len() != 3 {
        return date.to_string();
    }
    let month: usize = parts[1].parse().unwrap_or(0);
    let months = defs::strings(engine("MONTH_NAMES"));
    let day = parts[2].trim_start_matches('0');
    format!("{} {}, {}", months.get(month).map(String::as_str).unwrap_or(""), day, parts[0])
}

/// `DrawFrame`: corners 249, lines 196 and 179, in the default
/// foreground.
pub fn draw_frame(c: &mut Console, x: i32, y: i32, w: i32, h: i32) {
    for (cx, cy) in [(x, y), (x + w - 1, y), (x, y + h - 1), (x + w - 1, y + h - 1)] {
        c.put_char(cx, cy, 249);
    }
    for x1 in x + 1..x + w - 1 {
        c.put_char(x1, y, 196);
        c.put_char(x1, y + h - 1, 196);
    }
    for y1 in y + 1..y + h - 1 {
        c.put_char(x, y1, 179);
        c.put_char(x + w - 1, y1, 179);
    }
}

/// `NewConsole`
pub fn new_console(w: i32, h: i32, bg: Color, fg: Color) -> Console {
    let mut c = Console::new(w, h);
    c.bg = bg;
    c.fg = fg;
    c.clear();
    c
}

/// Print a key and its command: the key in `ACTION_KEY_COL`, the text
/// after it at `tx`.
pub fn key_line(c: &mut Console, x: i32, tx: i32, y: i32, key: &str, label: &str, label_col: Color) {
    c.fg = ACTION_KEY_COL;
    c.print_ex(x, y, Align::Left, key);
    c.fg = label_col;
    c.print_ex(tx, y, Align::Left, label);
}

pub struct Game {
    pub rng: Rng,
    pub scr: Screen,
    /// libtcod's root console, the screen.
    pub root: Console,
    /// The page the menus and windows are drawn on (`con`).
    pub con: Console,
    pub settings: Settings,
    keyboard_decode: HashMap<String, String>,
    keyboard_encode: HashMap<String, String>,
    pub campaign: Option<Campaign>,
    pub campaign_day: Option<CampaignDay>,
    /// `session.exiting`: the player saved and quit to the main menu.
    pub exiting: bool,
    /// The screen was resized: the loop under way should draw again.
    redraw: bool,
    /// Headless runs (tests): the keys to press, in turn.
    pub script: std::collections::VecDeque<Key>,
    /// The day map, while its screen is up.
    pub day_map: Option<crate::daymap::DayMap>,
    pub day_anim: crate::daymap::DayAnim,
    /// A message window over the day map: console and place.
    pub msg: Option<(Console, i32, i32)>,
    /// The mouse has moved over the screen (so the zone info follows it).
    pub mouse_seen: bool,
    /// The battle under way, if any (the original's `scenario`).
    pub scenario: Option<crate::battle::Scenario>,
    pub battle_anim: crate::battle_ui::BattleAnim,
    /// Tests: draw the screens even without a terminal.
    pub render_headless: bool,
    /// Nothing is saved (`--battle`).
    pub no_saves: bool,
}

impl Game {
    pub fn new(scr: Screen, rng: Rng) -> Self {
        let (w, h) = scr.size();
        if w > 0 {
            set_screen_size(w, h);
        }
        let settings = Settings::load();
        crate::sound::configure(settings.sounds_enabled, settings.master_volume);
        let mut g = Game {
            rng,
            scr,
            root: new_console(sw(), sh(), color::BLACK, color::WHITE),
            con: new_console(PAGE_W, PAGE_H, color::BLACK, color::WHITE),
            settings,
            keyboard_decode: HashMap::new(),
            keyboard_encode: HashMap::new(),
            campaign: None,
            campaign_day: None,
            exiting: false,
            redraw: false,
            script: std::collections::VecDeque::new(),
            day_map: None,
            day_anim: crate::daymap::DayAnim::default(),
            msg: None,
            mouse_seen: false,
            scenario: None,
            battle_anim: crate::battle_ui::BattleAnim::default(),
            render_headless: false,
            no_saves: false,
        };
        g.generate_keyboards();
        g
    }

    pub fn camp(&self) -> &Campaign {
        self.campaign.as_ref().expect("a campaign")
    }

    pub fn campm(&mut self) -> &mut Campaign {
        self.campaign.as_mut().expect("a campaign")
    }

    pub fn cd(&self) -> &CampaignDay {
        self.campaign_day.as_ref().expect("a campaign day")
    }

    pub fn cdm(&mut self) -> &mut CampaignDay {
        self.campaign_day.as_mut().expect("a campaign day")
    }

    // ---------------------------------------------------------------------
    // keys

    /// `GenerateKeyboards`
    pub fn generate_keyboards(&mut self) {
        self.keyboard_decode.clear();
        self.keyboard_encode.clear();
        let name = KEYBOARDS.get(self.settings.keyboard).copied().unwrap_or("QWERTY");
        if let Some(map) = defs::defs().keyboards.get(name).and_then(|m| m.as_object()) {
            for (k, v) in map {
                self.keyboard_decode.insert(k.clone(), text(v).to_string());
                self.keyboard_encode.insert(text(v).to_string(), k.clone());
            }
        }
    }

    /// `DeKey`: a key pressed on the player's layout to the standard one.
    pub fn dekey(&self, c: char) -> char {
        self.keyboard_decode.get(&c.to_string()).and_then(|s| s.chars().next()).unwrap_or(c)
    }

    /// `EnKey`: a standard key to the one on the player's layout.
    pub fn enkey(&self, c: char) -> char {
        self.keyboard_encode.get(&c.to_string()).and_then(|s| s.chars().next()).unwrap_or(c)
    }

    /// "W/S" on the player's layout.
    pub fn enkeys(&self, a: char, b: char) -> String {
        format!("{}/{}", self.enkey(a).to_ascii_uppercase(), self.enkey(b).to_ascii_uppercase())
    }

    pub fn key(&self) -> Key {
        self.scr.key
    }

    /// `chr(key.c).lower()`
    pub fn key_char(&self) -> char {
        self.scr.key.c.to_ascii_lowercase()
    }

    /// `DeKey(chr(key.c).lower())`
    pub fn mapped_char(&self) -> char {
        self.dekey(self.key_char())
    }

    // ---------------------------------------------------------------------
    // the screen

    /// `console_flush`
    pub fn flush(&mut self) {
        self.scr.flush(&self.root);
    }

    /// `GetInputEvent`: true if a key was pressed. A resize lays the
    /// screen out again and asks the loop under way to draw again
    /// (`take_redraw`).
    pub fn input(&mut self) -> bool {
        if self.scr.is_headless() {
            let k = self.script.pop_front().expect("headless: a screen is waiting for a key and none is scripted");
            self.scr.key = k;
            return true;
        }
        self.scr.check_for_event();
        if let Some((w, h)) = self.scr.resized.take() {
            set_screen_size(w, h);
            log::info!("resize: terminal {w}x{h}, screen {}x{}", sw(), sh());
            self.root = new_console(sw(), sh(), color::BLACK, color::WHITE);
            self.redraw = true;
        }
        self.scr.key.vk != Vk::None
    }

    /// True once after the screen was resized.
    pub fn take_redraw(&mut self) -> bool {
        std::mem::take(&mut self.redraw)
    }

    /// Show the page (`con`) on the screen: `console_blit(con, ..., 0, 0, 0)`.
    pub fn show_page(&mut self) {
        self.root.bg = color::BLACK;
        self.root.clear();
        Console::blit(&self.con, 0, 0, PAGE_W, PAGE_H, &mut self.root, page_x(), page_y());
    }

    /// `Wait`, in hundredths of a second as the original's; Enter skips it
    /// if allowed. Headless runs do not wait.
    pub fn wait(&mut self, hundredths: u64, allow_skip: bool) {
        if self.scr.is_headless() {
            return;
        }
        let until = std::time::Instant::now() + std::time::Duration::from_millis(hundredths * 10);
        while std::time::Instant::now() < until {
            self.day_tick();
            self.battle_tick();
            self.flush();
            if !self.input() {
                continue;
            }
            if allow_skip && self.key().vk == Vk::Enter {
                return;
            }
        }
    }

    /// `WaitForContinue`: wait for Enter; with `allow_cancel`, Backspace
    /// cancels and returns true.
    pub fn wait_for_continue(&mut self, allow_cancel: bool) -> bool {
        if self.scr.is_headless() && self.script.is_empty() {
            return false;
        }
        loop {
            self.flush();
            if !self.input() {
                continue;
            }
            match self.key().vk {
                Vk::Backspace if allow_cancel => return true,
                Vk::Enter => return false,
                _ => {}
            }
        }
    }

    /// `ShowMessage`, in the middle of the screen: a framed window, shown
    /// for a moment and then taken away.
    pub fn show_message(&mut self, text: &str, longer_pause: bool, portrait: Option<&str>) {
        self.show_message_at(text, longer_pause, portrait, None);
    }

    /// `ShowMessage` near a place on the screen (`x`, `y`), below it if
    /// `below` else above, as the original places messages about a zone.
    pub fn show_message_at(&mut self, text: &str, longer_pause: bool, portrait: Option<&str>, near: Option<(i32, i32, bool)>) {
        log::info!("message: {text}");
        let (width, mut height) = if portrait.is_some() { (27, 8) } else { (21, 4) };
        let lines = wrap(text, (width - 4) as usize, "");
        height += lines.len() as i32;
        let (x, y) = match near {
            Some((nx, ny, below)) => {
                let x = (nx - width / 2).clamp(0, sw() - width);
                let y = if below { ny + 3 } else { ny - height - 2 };
                (x, y.clamp(0, sh() - height))
            }
            None => (sxm() + 1 - width / 2, sh() / 2 - height / 2),
        };
        let mut msg = new_console(width, height, color::DARKEST_GREY, color::WHITE);
        draw_frame(&mut msg, 0, 0, width, height);
        let mut ly = height / 2 - lines.len() as i32 / 2;
        if let Some(p) = portrait.and_then(crate::xp::by_name) {
            msg.bg = PORTRAIT_BG_COL;
            msg.rect(1, 1, 25, 4, true);
            crate::art::draw(&p, &mut msg, 1, 1, 24, 4);
            msg.bg = color::BLACK;
            ly += 2;
        }
        for line in &lines {
            msg.print_name(width / 2, ly, Align::Center, line);
            if ly == height - 1 {
                break;
            }
            ly += 1;
        }
        let pause = if longer_pause { 200 } else { 130 };
        let pause = pause + 40 * self.settings.message_pause.max(0) as u64;
        if self.battle_anim.active && self.scenario.is_some() {
            self.msg = Some((msg, x, y));
            self.draw_battle();
            self.wait(pause, false);
            self.msg = None;
            self.draw_battle();
        } else if self.day_map.is_some() {
            self.msg = Some((msg, x, y));
            self.draw_day();
            self.wait(pause, false);
            self.msg = None;
            self.draw_day();
        } else {
            let saved = self.root.clone();
            Console::blit(&msg, 0, 0, width, height, &mut self.root, x, y);
            self.wait(pause, false);
            self.root = saved;
        }
        self.flush();
    }

    /// `ShowNotification`: a pop-up over the darkened screen, until Enter,
    /// or Y or N with `confirm` (true for Y).
    pub fn show_notification(&mut self, text: &str, confirm: bool) -> bool {
        log::info!("notification: {text}");
        if self.scr.is_headless() && self.script.is_empty() {
            return true;
        }
        let saved = self.root.clone();
        loop {
            let lines = wrap(text, 56, "");
            let h = lines.len() as i32 + 6;
            let x = sxm() - 30;
            let y = sh() / 2 - h / 2;
            self.root.darken(0.5);
            self.root.bg = color::BLACK;
            self.root.fg = color::WHITE;
            self.root.rect(x, y, 60, h, true);
            draw_frame(&mut self.root, x, y, 60, h);
            for (i, line) in lines.iter().enumerate() {
                self.root.print_name(x + 2, y + 2 + i as i32, Align::Left, line);
            }
            let prompt = if confirm { "Proceed? Y/N" } else { "Enter to Continue" };
            self.root.print_ex(sxm(), y + h - 2, Align::Center, prompt);
            loop {
                self.flush();
                if self.take_redraw() {
                    break;
                }
                if !self.input() {
                    continue;
                }
                let k = self.key_char();
                let answer = if confirm {
                    match k {
                        'y' => Some(true),
                        'n' => Some(false),
                        _ => None,
                    }
                } else {
                    (self.key().vk == Vk::Enter).then_some(true)
                };
                if let Some(a) = answer {
                    self.root = saved;
                    return a;
                }
            }
            // resized: the screen under the pop-up is drawn again by the
            // loop that called it; start from a cleared screen
            self.root = new_console(sw(), sh(), color::BLACK, color::WHITE);
        }
    }

    /// `ShowTextInputMenu`: a window to type a string in, up to
    /// `max_length`; Tab picks one from `string_list` if any. Escape gives
    /// back the original.
    pub fn show_text_input(&mut self, prompt: &str, original: &str, max_length: usize, string_list: &[String]) -> String {
        if self.scr.is_headless() && self.script.is_empty() {
            return original.to_string();
        }
        let saved = self.root.clone();
        let mut text = original.to_string();
        let (w, h) = (34, 12);
        let draw = |g: &mut Game, text: &str| {
            let (x, y) = (sxm() - w / 2, sh() / 2 - h / 2);
            let r = &mut g.root;
            r.bg = color::BLACK;
            r.fg = color::WHITE;
            r.rect(x, y, w, h, true);
            draw_frame(r, x, y, w, h);
            r.fg = color::LIGHT_GREY;
            let lines = wrap(prompt, 24, "");
            for (i, line) in lines.iter().take(3).enumerate() {
                r.print_name(sxm(), y + 1 + i as i32, Align::Center, line);
            }
            r.rect(x + 1, y + 5, w - 2, 1, true);
            r.fg = color::WHITE;
            r.print_name(sxm(), y + 5, Align::Center, text);
            key_line(r, x + 4, x + 10, y + 7, "Esc", "Cancel", color::WHITE);
            key_line(r, x + 4, x + 10, y + 8, "Del", "Clear", color::WHITE);
            if !string_list.is_empty() {
                key_line(r, x + 4, x + 10, y + 9, "Tab", "Generate Random", color::WHITE);
            }
            key_line(r, x + 4, x + 10, y + 10, "Enter", "Confirm and Continue", color::WHITE);
        };
        self.root.darken(0.7);
        let darkened = self.root.clone();
        draw(self, &text);
        loop {
            self.flush();
            if self.take_redraw() {
                self.root = new_console(sw(), sh(), color::BLACK, color::WHITE);
                draw(self, &text);
            }
            if !self.input() {
                continue;
            }
            let k = self.key();
            match k.vk {
                Vk::Escape => {
                    self.root = saved;
                    return original.to_string();
                }
                Vk::Enter => break,
                Vk::Tab => {
                    if string_list.is_empty() {
                        continue;
                    }
                    text = self.rng.choice(string_list).clone();
                }
                Vk::Delete => text.clear(),
                Vk::Backspace => {
                    if text.pop().is_none() {
                        continue;
                    }
                }
                _ => {
                    if k.c == '\0' || text.chars().count() >= max_length || !(' '..='~').contains(&k.c) {
                        continue;
                    }
                    text.push(k.c);
                }
            }
            if self.root.w == darkened.w && self.root.h == darkened.h {
                self.root = darkened.clone();
            }
            draw(self, &text);
        }
        self.root = saved;
        text
    }

    // ---------------------------------------------------------------------
    // options and the game menu

    /// `DisplayGameOptions`
    pub fn display_game_options(&self, c: &mut Console, x: i32, y: i32, skip_esc: bool) {
        draw_game_options(&self.settings, c, x, y, skip_esc);
    }

    /// `ChangeGameSettings`
    pub fn change_game_settings(&mut self, key_char: char) {
        if self.settings.change(key_char) {
            self.generate_keyboards();
        }
    }

    /// `ShowGameMenu`: commands and options over the darkened screen.
    /// Q saves and quits to the main menu (`exiting`).
    pub fn show_game_menu(&mut self) {
        if self.scr.is_headless() && self.script.is_empty() {
            return;
        }
        let saved = self.root.clone();
        let mut darkened = self.root.clone();
        darkened.darken(0.7);
        let (w, h) = (60, 28);
        loop {
            self.root = darkened.clone();
            if self.root.w != sw() || self.root.h != sh() {
                self.root = new_console(sw(), sh(), color::BLACK, color::WHITE);
            }
            let mut m = new_console(w, h, color::BLACK, color::WHITE);
            draw_frame(&mut m, 0, 0, w, h);
            m.bg = color::DARKER_GREY;
            m.rect(17, 1, 26, 5, true);
            m.bg = color::BLACK;
            draw_frame(&mut m, 17, 1, 26, 5);
            m.print_ex(30, 2, Align::Center, NAME);
            m.print_ex(30, 4, Align::Center, VERSION);
            for (y, title) in [(7, "Game Commands"), (12, "Game Options")] {
                m.bg = color::DARKEST_BLUE;
                m.rect(17, y, 26, 1, false);
                m.bg = color::DARKER_BLUE;
                m.rect(19, y, 22, 1, false);
                m.bg = color::BLACK;
                m.fg = color::WHITE;
                m.print_ex(30, y, Align::Center, title);
            }
            key_line(&mut m, 17, 22, 9, "Esc", "Close Menu", color::LIGHTER_GREY);
            key_line(&mut m, 17, 22, 10, "Q", "Save and Quit", color::LIGHTER_GREY);
            self.display_game_options(&mut m, 15, 14, true);
            m.fg = color::GREY;
            for (y, line) in [
                (20, "We are the Dead. Short days ago"),
                (21, "We lived, felt dawn, saw sunset glow,"),
                (22, "Loved and were loved, and now we lie"),
                (23, "In Flanders fields."),
                (25, "John McCrae (1872-1918)"),
            ] {
                m.print_ex(12, y, Align::Left, line);
            }
            Console::blit(&m, 0, 0, w, h, &mut self.root, sxm() - w / 2, sh() / 2 - h / 2);
            loop {
                self.flush();
                if self.take_redraw() {
                    darkened = new_console(sw(), sh(), color::BLACK, color::WHITE);
                    break;
                }
                if !self.input() {
                    continue;
                }
                if self.key().vk == Vk::Escape {
                    self.root = saved;
                    return;
                }
                if self.key_char() == 'q' {
                    crate::save::save_game(self);
                    self.exiting = true;
                    self.root = saved;
                    return;
                }
                let c = self.mapped_char();
                self.change_game_settings(c);
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(date_text("1939.09.01"), "September 1, 1939");
        assert_eq!(date_text("1944.12.25"), "December 25, 1944");
    }

    #[test]
    fn keyboards_map_both_ways() {
        let mut g = Game::new(Screen::headless(), Rng::new(1));
        assert_eq!(g.dekey('w'), 'w');
        g.settings.keyboard = 1; // AZERTY
        g.generate_keyboards();
        assert_eq!(g.dekey('z'), 'w');
        assert_eq!(g.enkey('w'), 'z');
    }
}

/// A key press, for scripts.
pub fn press(c: char) -> Key {
    let vk = match c {
        '\r' => Vk::Enter,
        '\u{1b}' => Vk::Escape,
        '\u{8}' => Vk::Backspace,
        ' ' => Vk::Space,
        '0'..='9' => Vk::Digit(c as u8 - b'0'),
        _ => Vk::Char,
    };
    Key { vk, c, shift: c.is_ascii_uppercase(), ctrl: false }
}
