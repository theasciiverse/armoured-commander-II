//! The original's `Campaign` class: the campaign's state, its combat
//! calendar, and the screens that start one (`CampaignSelectionMenu`,
//! `TankSelectionMenu`).

use crate::color;
use crate::console::{Align, Console};
use crate::defs::{self, Dict, engine, int, text};
use crate::game::*;
use crate::layout::*;
use crate::model::{self, CrewContext, Personnel, Unit, days_in_month};
use crate::ui::{Vk, wrap};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Options {
    /// "Player Commander": the campaign ends with the commander.
    pub permadeath: bool,
    pub fate_points: bool,
}

pub type Records = BTreeMap<String, i32>;

pub fn new_records() -> Records {
    defs::strings(engine("RECORD_LIST")).into_iter().map(|r| (r, 0)).collect()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Campaign {
    /// The campaign file's name without `.json`.
    pub filename: String,
    pub options: Options,
    /// The day's records, for each combat day.
    pub logs: Vec<(String, Records)>,
    /// The journal: (day, [(time, text)]) in the order the days came.
    pub journal: Vec<(String, Vec<(String, String)>)>,
    pub player_unit: Option<Unit>,
    pub player_squad_max: i32,
    pub player_vp: i32,
    /// The campaign file, which the rules read (and the week's modifiers
    /// change).
    pub stats: Dict,
    pub combat_calendar: Vec<String>,
    pub today: String,
    /// An index into `stats["calendar_weeks"]`.
    pub current_week: usize,
    pub hospital: Vec<Personnel>,
    pub active_calendar_menu: i32,
    pub active_journal_day: Option<String>,
    pub journal_scroll_line: i32,
    pub ended: bool,
    pub player_oob: bool,
    pub decoration: String,
    pub records: Records,
}

impl Campaign {
    pub fn new() -> Self {
        Campaign {
            filename: String::new(),
            options: Options { permadeath: true, fate_points: true },
            logs: Vec::new(),
            journal: Vec::new(),
            player_unit: None,
            player_squad_max: 0,
            player_vp: 0,
            stats: Dict::new(),
            combat_calendar: Vec::new(),
            today: String::new(),
            current_week: 0,
            hospital: Vec::new(),
            active_calendar_menu: 1,
            active_journal_day: None,
            journal_scroll_line: 0,
            ended: false,
            player_oob: false,
            decoration: String::new(),
            records: new_records(),
        }
    }

    pub fn pu(&self) -> &Unit {
        self.player_unit.as_ref().expect("a player unit")
    }

    pub fn pum(&mut self) -> &mut Unit {
        self.player_unit.as_mut().expect("a player unit")
    }

    pub fn weeks(&self) -> &Vec<Value> {
        self.stats["calendar_weeks"].as_array().expect("calendar_weeks")
    }

    /// `campaign.current_week`
    pub fn week(&self) -> &Dict {
        self.weeks()[self.current_week].as_object().expect("a week")
    }

    pub fn refitting(&self) -> bool {
        self.week().contains_key("refitting")
    }

    pub fn name(&self) -> &str {
        text(&self.stats["name"])
    }

    pub fn national_skills(&self) -> Vec<String> {
        self.stats.get("national_skills").map(defs::strings).unwrap_or_default()
    }

    /// What a new crewman of the player's unit needs.
    pub fn crew_context<'a>(&'a self, skills: &'a [String], player: bool) -> CrewContext<'a> {
        CrewContext { player, national_skills: skills, today: &self.today }
    }

    /// `CheckForNationalSkill`: the player character has the skill.
    pub fn check_national_skill(&self, skill: &str) -> bool {
        let player = defs::strings(engine("PLAYER_POSITIONS"));
        self.pu()
            .positions
            .iter()
            .filter(|p| player.contains(&p.name))
            .filter_map(|p| p.crewman.as_ref())
            .any(|c| c.skills.iter().any(|s| s == skill))
    }

    /// `GenerateCombatCalendar`: roll the combat days from each week's
    /// days and combat chance, plus one day for each refitting week.
    pub fn generate_combat_calendar(&mut self, rng: &mut crate::rng::Rng) {
        self.combat_calendar.clear();
        let end_date = text(&self.stats["end_date"]).to_string();
        let mut possible: Vec<(String, i32)> = Vec::new();
        for week in self.weeks() {
            if week.get("refitting").is_some() {
                continue;
            }
            let chance = int(&week["combat_chance"]);
            let start = text(&week["start_date"]).to_string();
            possible.push((start.clone(), chance));
            let mut day = start;
            for _ in 0..6 {
                day = next_day(&day);
                if let Some(e) = week.get("end_date") {
                    if day.as_str() > text(e) {
                        break;
                    }
                }
                if day > end_date {
                    break;
                }
                possible.push((day.clone(), chance));
            }
        }
        let wanted = int(&self.stats["combat_days"]) as usize;
        while self.combat_calendar.len() < wanted && !possible.is_empty() {
            let i = rng.int(0, possible.len() as i32 - 1) as usize;
            if rng.int(1, 100) <= possible[i].1 {
                self.combat_calendar.push(possible.remove(i).0);
            }
        }
        for week in self.weeks().clone() {
            if week.get("refitting").is_some() {
                self.combat_calendar.push(text(&week["start_date"]).to_string());
            }
        }
        self.combat_calendar.sort();
    }

    /// `AwardVP`, the campaign's part (the day's is `CampaignDay::day_vp`).
    pub fn award_vp(&mut self, vp: i32) {
        self.player_vp += vp;
        for p in &mut self.pum().positions {
            if let Some(c) = &mut p.crewman {
                if c.alive {
                    c.award_exp(vp);
                }
            }
        }
    }

    /// `AddJournal`, with the day clock's time.
    pub fn add_journal(&mut self, time: String, entry: &str) {
        log::info!("journal: {} {time} {entry}", self.today);
        let today = self.today.clone();
        match self.journal.iter_mut().find(|(d, _)| *d == today) {
            Some((_, list)) => list.push((time, entry.to_string())),
            None => self.journal.push((today, vec![(time, entry.to_string())])),
        }
    }

    /// `CheckForNewWeek`: the promotion messages to show.
    pub fn check_for_new_week(&mut self, rng: &mut crate::rng::Rng) -> Vec<String> {
        let mut messages = Vec::new();
        let i = self.current_week + 1;
        if i >= self.weeks().len() {
            return messages;
        }
        if self.today.as_str() < text(&self.weeks()[i]["start_date"]) {
            return messages;
        }
        self.current_week = i;
        if let Some(Value::Object(m)) = self.week().get("enemy_class_odds_modifier").cloned() {
            if let Some(Value::Object(odds)) = self.stats.get_mut("enemy_unit_class_odds") {
                for (k, v) in m {
                    if odds.contains_key(&k) {
                        odds.insert(k, v);
                    }
                }
            }
        }
        for p in &mut self.pum().positions {
            let name = p.name.clone();
            if let Some(c) = &mut p.crewman {
                messages.extend(c.promotion_check(&name, rng));
            }
        }
        messages
    }
}

/// The day after "YYYY.MM.DD", as the original works it out.
pub fn next_day(date: &str) -> String {
    let mut parts = date.split('.').map(|s| s.parse::<i32>().unwrap_or(1));
    let (mut y, mut m, mut d) = (parts.next().unwrap_or(1939), parts.next().unwrap_or(1), parts.next().unwrap_or(1));
    if d == days_in_month(y, m) {
        d = 1;
        if m == 12 {
            m = 1;
            y += 1;
        } else {
            m += 1;
        }
    } else {
        d += 1;
    }
    format!("{y}.{m:02}.{d:02}")
}

/// Days from one date to another (`(b - a).days`).
pub fn days_between(a: &str, b: &str) -> i32 {
    fn ordinal(date: &str) -> i32 {
        let mut p = date.split('.').map(|s| s.parse::<i32>().unwrap_or(1));
        let (y, m, d) = (p.next().unwrap_or(1939), p.next().unwrap_or(1), p.next().unwrap_or(1));
        let mut n = d;
        for mm in 1..m {
            n += days_in_month(y, mm);
        }
        for yy in 1900..y {
            n += if days_in_month(yy, 2) == 29 { 366 } else { 365 };
        }
        n
    }
    ordinal(b) - ordinal(a)
}

/// A campaign file's summary for the selection menu.
pub(crate) struct CampaignInfo {
    pub filename: String,
    pub data: &'static Value,
}

impl CampaignInfo {
    fn s(&self, k: &str) -> &str {
        text(&self.data[k])
    }
}

impl Game {
    /// `CampaignSelectionMenu`: false if the player went back to the main
    /// menu.
    pub fn campaign_selection_menu(&mut self) -> bool {
        let mut list: Vec<CampaignInfo> = defs::defs()
            .campaigns
            .iter()
            .map(|(f, v)| CampaignInfo { filename: f.clone(), data: v })
            .collect();
        if list.is_empty() {
            self.show_notification("Error: No campaign files available!", false);
            return false;
        }
        list.sort_by(|a, b| a.s("start_date").cmp(b.s("start_date")));
        let mut selected = 0usize;
        if self.scr.is_headless() && self.script.is_empty() {
            // tests: a random campaign, as R
            self.script.push_back(press('r'));
        }
        self.draw_campaign_selection(&list, selected);
        loop {
            self.flush();
            if self.take_redraw() {
                self.draw_campaign_selection(&list, selected);
            }
            if !self.input() {
                continue;
            }
            if self.key().vk == Vk::Escape {
                return false;
            }
            if self.key().vk == Vk::Enter {
                if crate::save::exists(&list[selected].filename)
                    && !self.show_notification(
                        "WARNING: Saved campaign already exists for this campaign, erase and start a new one?",
                        true,
                    )
                {
                    self.draw_campaign_selection(&list, selected);
                    continue;
                }
                break;
            }
            let (k, kc) = (self.mapped_char(), self.key_char());
            match k {
                '1' => self.campm().options.permadeath ^= true,
                '2' => self.campm().options.fate_points ^= true,
                's' => selected = (selected + 1) % list.len(),
                'w' => selected = (selected + list.len() - 1) % list.len(),
                _ if kc == 'r' => {
                    let old = selected;
                    selected = self.rng.int(0, list.len() as i32 - 1) as usize;
                    if crate::save::exists(&list[selected].filename) {
                        let text = format!(
                            "WARNING: Saved campaign already exists for {}, erase and start a new one?",
                            list[selected].s("name")
                        );
                        if !self.show_notification(&text, true) {
                            selected = old;
                            self.draw_campaign_selection(&list, selected);
                            continue;
                        }
                    }
                    break;
                }
                _ => continue,
            }
            self.draw_campaign_selection(&list, selected);
        }
        let chosen = &list[selected];
        log::info!("campaign: {}", chosen.filename);
        let filename = chosen.filename.clone();
        let stats = chosen.data.as_object().cloned().unwrap_or_default();
        let camp = self.campm();
        camp.filename = filename;
        camp.stats = stats;
        let mut rng = std::mem::replace(&mut self.rng, crate::rng::Rng::new(1));
        self.campm().generate_combat_calendar(&mut rng);
        self.rng = rng;
        let camp = self.campm();
        camp.today = camp.combat_calendar.first().cloned().unwrap_or_default();
        camp.current_week = 0;
        true
    }

    /// `UpdateCampaignSelectionScreen`, on the 100x30 page: the campaigns
    /// on the left, the selected one in the middle (its flag at half size),
    /// the options on the right.
    pub(crate) fn draw_campaign_selection(&mut self, list: &[CampaignInfo], selected: usize) {
        let c = &mut self.con;
        c.bg = color::BLACK;
        c.clear();
        let sel = &list[selected];

        // list of campaigns, two rows each
        c.fg = color::WHITE;
        let mut y = 2;
        for (i, camp) in list.iter().enumerate() {
            if i == selected {
                c.bg = color::DARK_BLUE;
                c.rect(1, y, 28, 2, true);
            }
            for (n, line) in wrap(camp.s("name"), 28, "").iter().take(2).enumerate() {
                c.print_name(1, y + n as i32, Align::Left, line);
            }
            c.bg = color::BLACK;
            y += 2;
            if y >= PAGE_H - 1 {
                break;
            }
        }

        // the selected campaign
        let (x0, w, xm) = (31, 39, 50);
        c.fg = color::WHITE;
        draw_frame(c, x0, 0, w, PAGE_H);
        c.fg = ACTION_KEY_COL;
        c.print_ex(xm, 1, Align::Center, "Campaign Selection");
        c.bg = color::DARKEST_GREY;
        c.rect(x0 + 1, 2, w - 2, 2, true);
        c.bg = color::BLACK;
        c.fg = PORTRAIT_BG_COL;
        for (n, line) in wrap(sel.s("name"), (w - 4) as usize, "").iter().take(2).enumerate() {
            c.print_name(xm, 2 + n as i32, Align::Center, line);
        }
        let nation = sel.s("player_nation");
        if let Some(flag) = defs::defs().nations.get(nation).and_then(|n| crate::xp::by_name(text(&n["flag_image"]))) {
            let (fw, fh) = crate::art::fit(&flag, w - 2, 7);
            crate::art::draw(&flag, c, xm - fw / 2, 4, fw, fh);
        }
        c.fg = color::WHITE;
        c.print_ex(xm, 11, Align::Center, "PLAYER FORCE");
        c.print_ex(xm, 13, Align::Center, "ENEMY FORCES");
        c.fg = color::LIGHT_GREY;
        c.print_name(xm, 12, Align::Center, nation);
        let enemies = defs::strings(&sel.data["enemy_nations"]).join(", ");
        c.print_name(xm, 14, Align::Center, &enemies);
        c.fg = color::WHITE;
        let dates =
            format!("{} to {}", date_text(sel.s("start_date")), date_text(sel.s("end_date")));
        c.print_ex(xm, 16, Align::Center, &dates);
        c.print_ex(xm, 17, Align::Center, &format!("Combat Days: {}", int(&sel.data["combat_days"])));
        c.fg = color::LIGHT_GREY;
        for (n, line) in wrap(sel.s("desc"), (w - 4) as usize, "").iter().take(6).enumerate() {
            c.print_name(x0 + 2, 19 + n as i32, Align::Left, line);
        }
        let we = self.enkeys('w', 's');
        let c = &mut self.con;
        key_line(c, x0 + 3, x0 + 10, 25, &we, "Select Campaign", color::WHITE);
        key_line(c, x0 + 3, x0 + 10, 26, "Enter", "Proceed", color::WHITE);
        key_line(c, x0 + 3, x0 + 10, 27, "R", "Proceed with Random", color::WHITE);
        key_line(c, x0 + 3, x0 + 10, 28, "Esc", "Return to Main Menu", color::WHITE);

        // campaign options
        let (ox, ow) = (72, 27);
        c.fg = color::LIGHT_GREEN;
        c.print_ex(ox, 1, Align::Left, "Campaign Options");
        c.fg = color::LIGHT_GREY;
        c.print_ex(ox, 2, Align::Left, "Press number to toggle");
        let opts = self.campaign.as_ref().map(|c| c.options.clone()).unwrap_or(Options { permadeath: true, fate_points: true });
        let mut y = 5;
        for (key, label, on, desc) in [
            (
                "1",
                "Player Commander",
                opts.permadeath,
                "You take on the role of the tank commander. If you are seriously injured, you may miss a number of days of the campaign. If you are killed, your campaign ends. ",
            ),
            (
                "2",
                "Fate Points",
                opts.fate_points,
                "You are protected by fate, negating a few incoming attacks per day that would otherwise destroy your tank.",
            ),
        ] {
            key_line(c, ox, ox + 2, y, key, label, if on { color::WHITE } else { color::DARKER_GREY });
            c.fg = color::LIGHT_GREY;
            y += 2;
            for line in wrap(desc, ow as usize, "") {
                c.print_ex(ox, y, Align::Left, &line);
                y += 1;
            }
            y += 2;
        }
        self.show_page();
    }

    /// `TankSelectionMenu`: the tank type and name chosen; `None` if the
    /// player kept the current tank (when replacing one).
    pub fn tank_selection_menu(&mut self, replacing_tank: bool) -> Option<(String, String)> {
        let camp = self.camp();
        let ids = defs::strings(&camp.stats["player_unit_list"]);
        let mut units: Vec<Unit> = Vec::new();
        for id in ids {
            // the original reads each type's rarity here and then makes no
            // use of it, so every tank in the list is offered (kept)
            if let Some(u) = Unit::new(&id, &mut self.rng) {
                units.push(u);
            }
        }
        if units.is_empty() {
            return None;
        }
        let can_keep = replacing_tank && self.camp().player_unit.as_ref().is_some_and(|u| u.alive);
        let mut selected = 0usize;
        let mut name = String::new();
        if self.scr.is_headless() && self.script.is_empty() {
            // tests: a random tank
            let i = self.rng.int(0, units.len() as i32 - 1) as usize;
            return Some((units[i].unit_id.clone(), name));
        }
        self.draw_tank_selection(&units, selected, &name, can_keep);
        loop {
            self.flush();
            if self.take_redraw() {
                self.draw_tank_selection(&units, selected, &name, can_keep);
            }
            if !self.input() {
                continue;
            }
            match self.key().vk {
                Vk::Enter => break,
                Vk::Backspace if can_keep => return None,
                _ => {}
            }
            if self.key_char() == 'n' {
                name = self.show_text_input("Enter a name for your tank", &name, MAX_TANK_NAME_LENGTH, &[]);
                self.draw_tank_selection(&units, selected, &name, can_keep);
                continue;
            }
            match self.mapped_char() {
                's' => selected = (selected + 1) % units.len(),
                'w' => selected = (selected + units.len() - 1) % units.len(),
                _ => continue,
            }
            self.draw_tank_selection(&units, selected, &name, can_keep);
        }
        log::info!("tank: {} \"{name}\"", units[selected].unit_id);
        Some((units[selected].unit_id.clone(), name))
    }

    /// `UpdateTankSelectionScreen`, on the 100x30 page.
    pub(crate) fn draw_tank_selection(&mut self, units: &[Unit], selected: usize, name: &str, can_keep: bool) {
        let enkeys = self.enkeys('w', 's');
        let c = &mut self.con;
        c.bg = color::BLACK;
        c.clear();
        let (x0, w, xm) = (31, 39, 50);
        if can_keep {
            c.fg = color::WHITE;
            c.print_ex(85, 1, Align::Center, "Current Tank");
            if let Some(u) = &self.campaign.as_ref().and_then(|c| c.player_unit.as_ref()) {
                display_my_info(u, c, 73, 3, false);
            }
            c.fg = color::DARK_GREY;
            c.hline(72, 15, 27);
        }
        c.fg = color::WHITE;
        draw_frame(c, x0, 0, w, PAGE_H);
        c.bg = color::DARKER_BLUE;
        c.rect(x0 + 1, 1, w - 2, 1, false);
        c.bg = color::BLACK;
        c.fg = ACTION_KEY_COL;
        c.print_ex(xm, 1, Align::Center, "Player Tank Selection");
        c.fg = color::WHITE;
        c.print_ex(xm, 2, Align::Center, "Select a tank type to command");

        // the tank types, one a row (a column of them if there are many)
        let rows = PAGE_H - 2;
        for (i, u) in units.iter().enumerate() {
            let (x, y) = (1 + (i as i32 / rows) * 15, 1 + i as i32 % rows);
            if x > x0 - 10 {
                break;
            }
            let cols = if units.len() as i32 > rows { 14 } else { 28 };
            if i == selected {
                c.bg = color::DARKER_BLUE;
                c.rect(x, y, cols, 1, false);
                c.bg = color::BLACK;
            }
            c.fg = color::WHITE;
            c.print_name(x, y, Align::Left, &u.unit_id);
        }

        let u = &units[selected];
        c.fg = color::WHITE;
        display_my_info(u, c, x0 + 7, 4, false);
        c.fg = color::WHITE;
        c.print_name(x0 + 7, 6, Align::Left, name);
        c.print_ex(x0 + 7, 15, Align::Left, &format!("Crew: {}", u.positions.len()));
        // the description to the right, under the current tank if any
        let desc: String = u.stats.get("description").map(defs::strings).unwrap_or_default().concat();
        let top = if can_keep { 16 } else { 1 };
        c.fg = color::LIGHT_GREY;
        for (n, line) in wrap(&desc, 27, "").iter().take((PAGE_H - 1 - top) as usize).enumerate() {
            c.print_name(72, top + n as i32, Align::Left, line);
        }
        key_line(c, x0 + 3, x0 + 10, 25, &enkeys, "Select Unit Type", color::WHITE);
        key_line(c, x0 + 3, x0 + 10, 26, "N", "Set Tank Name", color::WHITE);
        if can_keep {
            key_line(c, x0 + 3, x0 + 10, 27, "Bksp", "Keep Current Tank", color::WHITE);
        }
        key_line(c, x0 + 3, x0 + 10, 28, "Enter", "Proceed", color::WHITE);
        self.show_page();
    }

    /// The new campaign from the main menu: choose it and the tank, crew
    /// the tank, and set up the first day (the original's main loop).
    /// False if the player went back.
    pub fn start_new_campaign(&mut self) -> bool {
        self.campaign = Some(Campaign::new());
        if !self.campaign_selection_menu() {
            self.campaign = None;
            return false;
        }
        let Some((unit_id, tank_name)) = self.tank_selection_menu(false) else {
            self.campaign = None;
            return false;
        };
        let Some(mut unit) = Unit::new(&unit_id, &mut self.rng) else {
            self.campaign = None;
            return false;
        };
        unit.unit_name = tank_name;
        unit.nation = text(&self.camp().stats["player_nation"]).to_string();
        let skills = self.camp().national_skills();
        let today = self.camp().today.clone();
        let ctx = CrewContext { player: true, national_skills: &skills, today: &today };
        unit.generate_new_personnel(&ctx, &mut self.rng);
        unit.clear_gun_ammo();
        self.campm().player_unit = Some(unit);
        self.new_campaign_day();
        self.add_journal("Start of day");
        true
    }

    /// `AddJournal`: an entry at the day clock's time.
    pub fn add_journal(&mut self, entry: &str) {
        let Some(cd) = &self.campaign_day else { return };
        let time = format!("{:02}:{:02}", cd.day_clock.0, cd.day_clock.1);
        self.campm().add_journal(time, entry);
    }
}

/// `Unit.DisplayMyInfo`, in 25 columns: name and class, the portrait at
/// half its size (25x4), weapons, armour and movement; with `status`, the
/// unit's status and terrain under them. 11 rows, 15 with status.
pub fn display_my_info(u: &Unit, c: &mut Console, x: i32, y: i32, status: bool) {
    c.bg = color::BLACK;
    c.fg = color::LIGHTER_BLUE;
    c.print_name(x, y, Align::Left, &u.unit_id);
    if !u.nick_name.is_empty() {
        c.fg = color::WHITE;
        c.print_name(x + 24, y, Align::Right, &u.nick_name);
    }
    c.fg = color::LIGHT_GREY;
    c.print_ex(x, y + 1, Align::Left, u.stat("class").unwrap_or(""));

    c.bg = PORTRAIT_BG_COL;
    c.rect(x, y + 2, 25, 4, true);
    if let Some(p) = u.stat("portrait").and_then(crate::xp::by_name) {
        let (pw, ph) = crate::art::fit(&p, 25, 4);
        crate::art::draw(&p, c, x, y + 2, pw, ph);
    }
    if !u.unit_name.is_empty() {
        c.fg = color::WHITE;
        c.print_name(x, y + 2, Align::Left, &u.unit_name);
    }

    // weapons: turret mounts on the first line, all others on the second
    c.bg = color::DARKEST_RED;
    c.rect(x, y + 6, 25, 2, true);
    let (mut x1, mut x2) = (x, x);
    for w in &u.weapons {
        c.fg = if w.broken {
            color::DARKEST_GREY
        } else if w.jammed {
            color::LIGHT_RED
        } else if defs::get(&w.stats, "unreliable").is_some() {
            color::LIGHT_GREY
        } else {
            color::WHITE
        };
        let turret = w.stat("mount") == Some("Turret");
        let xx = if turret { &mut x1 } else { &mut x2 };
        let mut t = w.name().to_string();
        if *xx != x {
            t = format!(", {t}");
        }
        c.print_ex(*xx, if turret { y + 6 } else { y + 7 }, Align::Left, &t);
        *xx += t.chars().count() as i32;
    }
    c.bg = color::BLACK;

    // armour
    c.fg = color::WHITE;
    match defs::get(&u.stats, "armour").and_then(|a| a.as_object()) {
        None => c.print_ex(x, y + 8, Align::Left, "Unarmoured"),
        Some(a) => {
            c.print_ex(x, y + 8, Align::Left, "Armoured");
            c.fg = color::LIGHT_GREY;
            let t = if u.has("turret") { "T" } else { "U" };
            let s = |k: &str| a.get(k).map(|v| text(v).to_string()).unwrap_or_default();
            c.print_ex(x + 1, y + 9, Align::Left, &format!("{t} {}/{}", s("turret_front"), s("turret_side")));
            c.print_ex(x + 1, y + 10, Align::Left, &format!("H {}/{}", s("hull_front"), s("hull_side")));
        }
    }
    if u.stat("turret") == Some("Fast") {
        c.print_ex(x + 8, y + 9, Align::Left, "(fast)");
    }

    // movement class
    let t = if u.immobilized {
        c.fg = color::LIGHT_RED;
        "Immobilized".to_string()
    } else if u.bogged {
        c.fg = color::LIGHT_RED;
        "Bogged Down".to_string()
    } else {
        c.fg = match u.stat("ground_pressure") {
            Some("Light") => color::LIGHTER_GREEN,
            Some("Heavy") => color::DARKER_GREEN,
            Some(_) => c.fg,
            None => color::GREEN,
        };
        let mut t = u.stat("movement_class").unwrap_or("").to_string();
        if u.has("powerful_engine") {
            t.push('+');
        }
        t
    };
    c.print_ex(x + 24, y + 8, Align::Right, &t);

    c.fg = color::DARK_GREEN;
    let mut extras = Vec::new();
    if u.has("HVSS") {
        extras.push("HVSS");
    }
    if u.has("recce") {
        extras.push("Recce");
    }
    if u.has("off_road") {
        extras.push("ATV");
    }
    c.print_ex(x + 24, y + 9, Align::Right, &extras.join(" "));

    c.fg = color::WHITE;
    if let Some(size) = u.stat("size_class") {
        if size != "Normal" {
            c.print_ex(x + 24, y + 10, Align::Right, size);
        }
    }

    if status {
        let ys = y + 11;
        if let Some(&d) = u.hull_down.first() {
            c.fg = color::SEPIA;
            c.print_ex(x + 8, ys - 1, Align::Left, "HD");
            c.put_char_ex(x + 10, ys - 1, crate::hexes::arrow(d), color::SEPIA, color::BLACK);
        } else if u.reduced {
            c.fg = color::LIGHT_RED;
            c.print_ex(x, ys - 1, Align::Left, "Reduced");
        }
        c.fg = color::LIGHT_GREY;
        c.bg = color::DARKEST_BLUE;
        c.rect(x, ys, 25, 2, true);
        let mut t = String::new();
        if u.routed {
            t += "Routed ";
        } else if u.pinned {
            t += "Pinned ";
        }
        if u.moving {
            t += "Moving ";
        }
        if u.fired {
            t += "Fired ";
        }
        c.print_ex(x, ys + 1, Align::Left, &t);
        c.bg = color::DARKER_SEPIA;
        c.rect(x, ys + 2, 25, 2, true);
        if let Some(terrain) = &u.terrain {
            c.fg = color::GREEN;
            c.print_ex(x, ys + 2, Align::Left, terrain);
        }
        if u.smoke > 0 {
            c.fg = color::GREY;
            c.print_ex(x + 24, ys + 2, Align::Right, &format!("Smoke {}", u.smoke));
        }
    }
    c.bg = color::BLACK;
}

/// Ensure the model's dice are reachable for code that only has `Game`.
pub fn percentile(g: &mut Game) -> f64 {
    model::percentile(&mut g.rng)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rng;
    use crate::ui::Screen;

    #[test]
    fn calendar_days() {
        assert_eq!(next_day("1939.09.30"), "1939.10.01");
        assert_eq!(next_day("1939.12.31"), "1940.01.01");
        assert_eq!(next_day("1940.02.28"), "1940.02.29");
        assert_eq!(days_between("1939.09.01", "1939.09.08"), 7);
        assert_eq!(days_between("1939.12.30", "1940.01.02"), 3);
    }

    #[test]
    fn every_campaign_rolls_its_calendar() {
        let mut rng = Rng::new(4);
        for (name, data) in &defs::defs().campaigns {
            let mut c = Campaign::new();
            c.stats = data.as_object().unwrap().clone();
            c.generate_combat_calendar(&mut rng);
            let refits = c.weeks().iter().filter(|w| w.get("refitting").is_some()).count();
            assert_eq!(c.combat_calendar.len(), int(&c.stats["combat_days"]) as usize + refits, "{name}");
            assert!(c.combat_calendar.windows(2).all(|w| w[0] <= w[1]));
        }
    }

    #[test]
    fn the_unit_info_fits_its_columns() {
        let mut g = Game::new(Screen::headless(), Rng::new(1));
        let mut con = Console::new(40, 20);
        for id in defs::defs().unit_types.keys() {
            let u = Unit::new(id, &mut g.rng).unwrap();
            con.clear();
            display_my_info(&u, &mut con, 0, 0, true);
        }
    }
}
