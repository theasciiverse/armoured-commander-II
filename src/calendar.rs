//! The campaign calendar (`DoCampaignCalendarLoop` and the `Campaign`
//! screens around it): the day outline, command menu and main panel,
//! moving from one combat day to the next, the field hospital, the crew
//! screens, the start and end of a day, a new tank, and the end of the
//! campaign.
//!
//! The calendar is laid out on the 100x30 page: the day outline and the
//! command menu in a 24-column left column, the main panel (73x28) to the
//! right, in the frame `calendar_bkg.xp` draws (lines only, drawn here).

use crate::campaign::{Records, days_between, display_my_info};
use crate::color::{self, Color};
use crate::console::{Align, Console};
use crate::defs::{self, engine, int, num, text};
use crate::game::*;
use crate::layout::*;
use crate::model::{CrewContext, NameForm, Personnel, Unit, exp_required_for, percentile, rank_name};
use crate::ui::{Vk, wrap};

/// The main panel's size and place on the page.
const PANEL_X: i32 = 26;
const PANEL_W: i32 = 73;
const PANEL_H: i32 = 28;

/// `DisplayCrew`, `w` columns wide: four rows a position (position and
/// location; name and CE/BU; nickname and condition; fatigue), the
/// original's five less its blank one, with the first name and surname on
/// one row.
pub fn display_crew(u: &Unit, c: &mut Console, x: i32, mut y: i32, w: i32, highlight: Option<usize>) {
    for (i, p) in u.positions.iter().enumerate() {
        if highlight == Some(i) {
            c.bg = color::DARKER_BLUE;
            c.rect(x, y, w, 4, true);
            c.bg = color::BLACK;
        }
        c.fg = color::LIGHT_BLUE;
        if p.crewman.as_ref().is_some_and(|cr| cr.normal_position != p.name) {
            c.fg = color::LIGHT_RED;
        }
        c.print_ex(x, y, Align::Left, &p.name);
        c.fg = color::WHITE;
        c.print_ex(x + w - 1, y, Align::Right, p.location.as_deref().unwrap_or(""));
        match &p.crewman {
            None => c.print_ex(x, y + 1, Align::Left, "Empty"),
            Some(cr) => {
                c.print_name(x, y + 1, Align::Left, &cr.name(NameForm::Full));
                c.print_ex(x + w - 1, y + 1, Align::Right, if cr.ce { "CE" } else { "BU" });
                if !cr.nickname.is_empty() {
                    c.print_name(x + 1, y + 2, Align::Left, &format!("\"{}\"", cr.nickname));
                }
                if cr.condition != "Good Order" {
                    c.fg = color::RED;
                    c.print_ex(x + w - 1, y + 2, Align::Right, &cr.condition);
                }
                if cr.fatigue > 0 {
                    c.fg = color::RED;
                    c.print_ex(x + w - 1, y + 3, Align::Right, &format!("Fatigued {}%", cr.fatigue));
                }
            }
        }
        c.fg = color::WHITE;
        y += 4;
    }
}

/// The four stat symbols, in `CREW_STATS` order.
const STAT_SYMBOLS: [(u16, Color); 4] = [(4, color::YELLOW), (3, color::RED), (5, color::BLUE), (6, color::GREEN)];

/// A crewman's stats, four rows: symbol, name, value.
fn draw_stats(c: &mut Console, x: i32, y: i32, cr: &Personnel) {
    for (i, t) in defs::strings(engine("CREW_STATS")).iter().enumerate() {
        let (ch, col) = STAT_SYMBOLS[i];
        let yy = y + i as i32;
        c.put_char_ex(x, yy, ch, col, c.char_bg(x, yy));
        c.fg = color::WHITE;
        c.print_ex(x + 2, yy, Align::Left, t);
        c.fg = color::LIGHT_GREY;
        c.print_ex(x + 14, yy, Align::Right, &cr.stat(t).to_string());
    }
}

/// The page's frame, as `calendar_bkg.xp` draws it: the outline, the left
/// column, and the line under the day outline.
fn draw_calendar_frame(c: &mut Console) {
    c.fg = color::WHITE;
    draw_frame(c, 0, 0, PAGE_W, PAGE_H);
    draw_frame(c, 0, 0, 26, PAGE_H);
    for x in 1..25 {
        c.put_char(x, 18, 196);
    }
    c.put_char(0, 18, 249);
    c.put_char(25, 18, 249);
}

impl Game {
    /// `UpdateDayOutlineCon`, 24x17 at (1, 1).
    fn draw_day_outline(&mut self) {
        let camp = self.campaign.as_ref().expect("a campaign");
        let c = &mut self.con;
        let (x0, y0) = (1, 1);
        c.fg = PORTRAIT_BG_COL;
        let lines = wrap(camp.name(), 20, "");
        let mut y = y0;
        for line in lines.iter().take(3) {
            c.print_name(x0 + 12, y, Align::Center, line);
            y += 1;
        }
        c.fg = color::DARK_GREY;
        for x in 0..24 {
            c.put_char(x0 + x, y0 + 3, b'-' as u16);
        }
        c.fg = color::LIGHT_GREY;
        c.print_ex(x0 + 11, y0 + 4, Align::Center, &date_text(&camp.today));
        let week = camp.week();
        if camp.refitting() {
            c.print_ex(x0 + 12, y0 + 6, Align::Center, "Refitting");
        } else {
            c.fg = color::WHITE;
            for (i, line) in wrap(text(&week["week_title"]), 20, "").iter().take(2).enumerate() {
                c.print_name(x0 + 12, y0 + 6 + i as i32, Align::Center, line);
            }
        }
        if let Some(d) = week.get("week_description") {
            c.fg = color::LIGHT_GREY;
            for (i, line) in wrap(text(d), 22, "").iter().take(5).enumerate() {
                c.print_name(x0 + 1, y0 + 9 + i as i32, Align::Left, line);
            }
        }
        c.fg = color::WHITE;
        c.print_ex(x0 + 2, y0 + 14, Align::Left, &format!("Campaign VP: {}", camp.player_vp));
        if camp.refitting() {
            return;
        }
        c.fg = color::LIGHT_GREY;
        c.print_ex(x0 + 1, y0 + 15, Align::Left, "Start of Day:");
        c.print_ex(x0 + 3, y0 + 16, Align::Left, "End of Day:");
        c.fg = color::WHITE;
        c.print_ex(x0 + 19, y0 + 15, Align::Right, text(&week["day_start"]));
        c.print_ex(x0 + 19, y0 + 16, Align::Right, text(&week["day_end"]));
    }

    /// `UpdateCalendarCmdCon`, 24x10 at (1, 19).
    fn draw_calendar_cmd(&mut self) {
        let keys_ws = self.enkeys('w', 's');
        let keys_ad = self.enkeys('a', 'd');
        let f = self.enkey('f').to_ascii_uppercase().to_string();
        let camp = self.campaign.as_ref().expect("a campaign");
        let started = self.campaign_day.as_ref().is_some_and(|d| d.started);
        let c = &mut self.con;
        let (x0, y0) = (1, 19);
        c.fg = TITLE_COL;
        c.print_ex(x0 + 6, y0, Align::Left, "Command Menu");
        c.fg = color::LIGHT_GREY;
        c.print_ex(x0, y0 + 8, Align::Left, "Use highlighted keys");
        c.print_ex(x0, y0 + 9, Align::Left, "to navigate interface");

        let mut x = x0;
        let mut last = color::BLACK;
        for item in engine("CC_MENU_LIST").as_array().into_iter().flatten() {
            let (label, n, col) = (text(&item[0]), int(&item[1]), &item[2]);
            let col = Color(int(&col[0]) as u8, int(&col[1]) as u8, int(&col[2]) as u8);
            last = col;
            c.bg = col;
            c.rect(x, y0 + 1, 2, 1, true);
            c.put_char_ex(x, y0 + 1, b'0' as u16 + n as u16, ACTION_KEY_COL, color::mul(col, color::LIGHT_GREY));
            x += 2;
            if camp.active_calendar_menu == n {
                c.rect(x, y0 + 1, label.len() as i32 + 2, 1, true);
                c.fg = color::WHITE;
                c.print_ex(x, y0 + 1, Align::Left, label);
                x += label.len() as i32 + 2;
            }
        }
        c.bg = last;
        c.rect(x, y0 + 1, x0 + 24 - x, 1, true);
        c.bg = color::BLACK;

        match camp.active_calendar_menu {
            1 => {
                let label = if started { "Next Day" } else { "Start Day" };
                key_line(c, x0 + 2, x0 + 9, y0 + 4, "Enter", label, color::LIGHT_GREY);
            }
            2 => {
                key_line(c, x0 + 2, x0 + 6, y0 + 3, &keys_ws, "Select Position", color::LIGHT_GREY);
                key_line(c, x0 + 2, x0 + 6, y0 + 4, "P", "Swap Position", color::LIGHT_GREY);
                key_line(c, x0 + 2, x0 + 6, y0 + 5, &f, "Crewman Menu", color::LIGHT_GREY);
                key_line(c, x0 + 2, x0 + 6, y0 + 6, "N", "Crewman Nickname", color::LIGHT_GREY);
                if camp.pu().unit_name.is_empty() {
                    key_line(c, x0 + 2, x0 + 6, y0 + 7, "T", "Set Tank Name", color::LIGHT_GREY);
                }
            }
            3 => {
                key_line(c, x0 + 2, x0 + 7, y0 + 4, &keys_ws, "Scroll Log", color::LIGHT_GREY);
                key_line(c, x0 + 2, x0 + 7, y0 + 5, &keys_ad, "Select Day", color::LIGHT_GREY);
            }
            _ => {
                key_line(c, x0 + 2, x0 + 7, y0 + 4, &keys_ws, "Select Crewman", color::LIGHT_GREY);
                key_line(c, x0 + 2, x0 + 7, y0 + 5, &f, "Crewman Menu", color::LIGHT_GREY);
            }
        }
    }

    /// `UpdateCCMainPanel`, 73x28 at (26, 1).
    fn draw_calendar_panel(&mut self, selected_position: usize, selected_hospital: usize) {
        let mut p = new_console(PANEL_W, PANEL_H, color::BLACK, color::WHITE);
        let camp = self.campaign.as_mut().expect("a campaign");
        let ended = self.campaign_day.as_ref().is_none_or(|d| d.ended);
        match camp.active_calendar_menu {
            1 => {
                if !ended {
                    let cd = self.campaign_day.as_ref().unwrap();
                    // sunrise gradient, the original's 58 rows in 28
                    for y in 0..PANEL_H {
                        let f = (y * 2) as f64 / WINDOW_HEIGHT as f64;
                        p.bg = Color((210.0 * f) as u8, (50.0 * f) as u8, 0);
                        p.rect(0, y, PANEL_W, 1, true);
                    }
                    let (x, y) = ((PANEL_W - 30) / 2, 5);
                    p.bg = color::BLACK;
                    p.rect(x - 3, y - 3, 36, 23, false);
                    p.fg = color::DARK_YELLOW;
                    draw_frame(&mut p, x - 3, y - 3, 36, 23);
                    p.bg = color::DARKER_BLUE;
                    p.rect(x, y - 1, 30, 3, false);
                    p.bg = color::BLACK;
                    p.fg = color::LIGHT_YELLOW;
                    p.print_ex(x + 15, y, Align::Center, "Today's Mission");
                    p.fg = color::LIGHT_BLUE;
                    p.print_ex(x + 15, y + 3, Align::Center, &cd.mission);
                    p.fg = color::LIGHT_GREY;
                    let desc = engine("MISSION_DESC").get(&cd.mission).map(text).unwrap_or("");
                    for (i, line) in wrap(desc, 30, "").iter().take(9).enumerate() {
                        p.print_ex(x, y + 5 + i as i32, Align::Left, line);
                    }
                    p.fg = color::WHITE;
                    p.print_ex(x, y + 15, Align::Left, "Air Support:");
                    p.print_ex(x, y + 16, Align::Left, "Artillery Support:");
                    p.fg = color::LIGHT_GREY;
                    let week = camp.week();
                    // `str()` of the value: Python shows a float as 80.0
                    let level = |k: &str| {
                        week.get(k).map_or_else(
                            || "None".to_string(),
                            |v| match v {
                                serde_json::Value::String(s) => s.clone(),
                                v if v.is_f64() => format!("{:?}", num(v)),
                                v => v.to_string(),
                            },
                        )
                    };
                    p.print_ex(x + 23, y + 15, Align::Left, &level("air_support_level"));
                    p.print_ex(x + 23, y + 16, Align::Left, &level("arty_support_level"));
                }
            }
            2 => {
                p.fg = TITLE_COL;
                p.print_ex(2, 1, Align::Left, "Tank Positions and Crewmen");
                p.print_ex(42, 1, Align::Left, "Player Tank");
                let u = camp.pu();
                display_crew(u, &mut p, 2, 3, 30, Some(selected_position));
                if u.alive {
                    display_my_info(u, &mut p, 42, 3, false);
                    let desc: String = u.stats.get("description").map(defs::strings).unwrap_or_default().concat();
                    p.fg = color::LIGHT_GREY;
                    for (i, line) in wrap(&desc, 29, "").iter().take(12).enumerate() {
                        p.print_name(42, 15 + i as i32, Align::Left, line);
                    }
                } else {
                    p.print_ex(42, 3, Align::Left, "None");
                }
            }
            3 => {
                if camp.active_journal_day.is_none() {
                    camp.active_journal_day = camp.journal.first().map(|(d, _)| d.clone());
                }
                let Some(day) = camp.active_journal_day.clone() else { return self.blit_panel(p) };
                p.bg = color::DARKER_GREY;
                p.rect(PANEL_W / 2 - 16, 0, 32, 3, false);
                p.bg = color::BLACK;
                p.fg = color::WHITE;
                p.print_ex(PANEL_W / 2, 1, Align::Center, &date_text(&day));
                let entries = camp.journal.iter().find(|(d, _)| *d == day).map(|(_, e)| e.clone()).unwrap_or_default();
                let mut y = 4;
                for (n, (time, entry)) in entries.iter().enumerate() {
                    if (n as i32) < camp.journal_scroll_line {
                        continue;
                    }
                    if y >= PANEL_H - 1 {
                        break;
                    }
                    p.fg = color::WHITE;
                    p.print_ex(2, y, Align::Left, time);
                    p.fg = color::LIGHT_GREY;
                    for line in wrap(entry, 58, " ") {
                        p.print_name(10, y, Align::Left, &line);
                        y += 1;
                    }
                }
            }
            _ => {
                p.bg = Color(200, 0, 0);
                p.rect(PANEL_W / 2 - 16, 0, 32, 3, false);
                p.bg = color::BLACK;
                p.fg = color::WHITE;
                p.print_ex(PANEL_W / 2, 1, Align::Center, "+ Field Hospital +");
                p.print_ex(2, 5, Align::Left, "Crewman");
                p.print_ex(26, 5, Align::Left, "Position");
                p.print_ex(PANEL_W - 3, 5, Align::Right, "Days until Recovery");
                p.fg = color::GREY;
                for x in 2..PANEL_W - 2 {
                    p.put_char(x, 6, b'-' as u16);
                }
                let mut y = 8;
                if camp.hospital.is_empty() {
                    p.fg = color::GREY;
                    p.print_ex(2, y, Align::Left, "No crewmen currently in hospital");
                } else {
                    p.fg = color::LIGHTER_GREY;
                    for (n, cr) in camp.hospital.iter().enumerate() {
                        if y >= PANEL_H - 1 {
                            break;
                        }
                        if n == selected_hospital {
                            p.bg = color::DARKER_BLUE;
                            p.rect(2, y, PANEL_W - 4, 1, false);
                            p.bg = color::BLACK;
                        }
                        p.print_name(2, y, Align::Left, &cr.name(NameForm::Initial));
                        p.print_ex(26, y, Align::Left, &cr.normal_position);
                        let (min, max) = cr.field_hospital.unwrap_or((0, 0));
                        let t = if min == 0 { format!("Maximum {max}") } else { format!("{min}-{max}") };
                        p.print_ex(PANEL_W - 13, y, Align::Right, &t);
                        y += 2;
                    }
                }
            }
        }
        self.blit_panel(p);
    }

    fn blit_panel(&mut self, p: Console) {
        Console::blit(&p, 0, 0, PANEL_W, PANEL_H, &mut self.con, PANEL_X, 1);
    }

    /// `UpdateCCDisplay`, with the three parts drawn again.
    fn draw_calendar(&mut self, selected_position: usize, selected_hospital: usize) {
        self.con.bg = color::BLACK;
        self.con.clear();
        draw_calendar_frame(&mut self.con);
        self.draw_day_outline();
        self.draw_calendar_cmd();
        self.draw_calendar_panel(selected_position, selected_hospital);
        self.show_page();
    }

    /// `DoCampaignCalendarLoop`, until the campaign ends or the player
    /// saves and quits.
    pub fn do_campaign_calendar_loop(&mut self) {
        let mut selected_position = 0usize;
        let mut selected_hospital = 0usize;
        let day_running = |g: &Game| g.campaign_day.as_ref().is_some_and(|d| d.started && !d.ended);
        if !day_running(self) {
            self.draw_calendar(selected_position, selected_hospital);
        }
        crate::save::save_game(self);
        loop {
            if day_running(self) {
                self.do_campaign_day_loop();
                if self.camp().ended {
                    self.log_day_records();
                    self.do_end();
                    return;
                }
                if self.exiting {
                    return;
                }
                self.show_end_of_day();
                self.draw_calendar(selected_position, selected_hospital);
            }
            self.flush();
            if self.take_redraw() {
                self.draw_calendar(selected_position, selected_hospital);
            }
            if self.scr.is_headless() && self.script.is_empty() {
                // tests: on through the calendar
                self.campm().active_calendar_menu = 1;
                self.script.push_back(press('\r'));
            }
            if !self.input() {
                continue;
            }
            if self.key().vk == Vk::Escape {
                self.show_game_menu();
                if self.exiting {
                    return;
                }
                self.draw_calendar(selected_position, selected_hospital);
                continue;
            }
            let k = self.mapped_char();
            if ('1'..='4').contains(&k) {
                let n = k as i32 - '0' as i32;
                if self.camp().active_calendar_menu != n {
                    self.campm().active_calendar_menu = n;
                    self.draw_calendar(selected_position, selected_hospital);
                }
                continue;
            }
            let kc = self.key_char();
            match self.camp().active_calendar_menu {
                1 => {
                    if self.key().vk != Vk::Enter {
                        continue;
                    }
                    if !self.cd().ended {
                        self.ammo_reload_menu();
                        self.cdm().started = true;
                        continue;
                    }
                    if self.next_calendar_day() {
                        return;
                    }
                    self.draw_calendar(selected_position, selected_hospital);
                }
                2 => {
                    let n = self.camp().pu().positions.len();
                    match k {
                        'w' => selected_position = (selected_position + n - 1) % n,
                        's' => selected_position = (selected_position + 1) % n,
                        'f' => {
                            if self.camp().pu().positions[selected_position].crewman.is_none() {
                                continue;
                            }
                            self.show_crewman_menu(CrewRef::Position(selected_position));
                        }
                        _ if kc == 'p' => self.show_swap_position_menu(),
                        _ if kc == 'n' => {
                            let Some(cr) = &self.camp().pu().positions[selected_position].crewman else { continue };
                            if !cr.nickname.is_empty() {
                                continue;
                            }
                            let nick = self.show_text_input(
                                "Enter a nickname; cannot be changed once set!",
                                "",
                                MAX_NICKNAME_LENGTH,
                                &[],
                            );
                            if !nick.is_empty() {
                                self.campm().pum().positions[selected_position].crewman.as_mut().unwrap().nickname = nick;
                            }
                        }
                        _ if kc == 't' => {
                            if !self.camp().pu().unit_name.is_empty() {
                                continue;
                            }
                            let name = self.show_text_input("Enter a name for your tank", "", MAX_TANK_NAME_LENGTH, &[]);
                            if !name.is_empty() {
                                self.campm().pum().unit_name = name;
                            }
                        }
                        _ => continue,
                    }
                    self.draw_calendar(selected_position, selected_hospital);
                }
                3 => {
                    let camp = self.campm();
                    let days: Vec<String> = camp.journal.iter().map(|(d, _)| d.clone()).collect();
                    match k {
                        'a' | 'd' => {
                            if days.is_empty() {
                                continue;
                            }
                            let i = camp.active_journal_day.as_ref().and_then(|d| days.iter().position(|x| x == d)).unwrap_or(0);
                            let i = if k == 'a' { (i + days.len() - 1) % days.len() } else { (i + 1) % days.len() };
                            camp.active_journal_day = Some(days[i].clone());
                            // the original keeps the scroll when changing day
                        }
                        'w' | 's' => {
                            // the original clamps to the entries less 50 (its
                            // panel's height), which below 50 entries put the
                            // scroll below zero, and scrolling up from the top
                            // crashed it; kept within the entries here
                            let len = camp
                                .active_journal_day
                                .as_ref()
                                .and_then(|d| camp.journal.iter().find(|(x, _)| x == d))
                                .map_or(0, |(_, e)| e.len() as i32);
                            camp.journal_scroll_line += if k == 's' { 1 } else { -1 };
                            camp.journal_scroll_line = camp.journal_scroll_line.min(len - 1).max(0);
                        }
                        _ => continue,
                    }
                    self.draw_calendar(selected_position, selected_hospital);
                }
                _ => {
                    let n = self.camp().hospital.len();
                    match k {
                        'w' if n > 0 => selected_hospital = (selected_hospital + n - 1) % n,
                        's' if n > 0 => selected_hospital = (selected_hospital + 1) % n,
                        // the original opened the menu of the first of no
                        // crewmen and crashed
                        'f' if selected_hospital < n => self.show_crewman_menu(CrewRef::Hospital(selected_hospital)),
                        _ => continue,
                    }
                    self.draw_calendar(selected_position, selected_hospital);
                }
            }
        }
    }

    /// Enter on the calendar after a day: on to the next combat day, or
    /// the end of the campaign (true).
    fn next_calendar_day(&mut self) -> bool {
        self.log_day_records();
        // the original takes the first of today's date in the calendar; a
        // campaign whose refitting week starts on a combat week's last day
        // (Canada's Best, 1945.03.04) has the date twice, and the next day
        // after the first is the second, so the calendar never moved on;
        // from the last of them here
        let camp = self.camp();
        let day_index = camp.combat_calendar.iter().rposition(|d| *d == camp.today).unwrap_or(0);
        if day_index == camp.combat_calendar.len() - 1 {
            self.do_end();
            return true;
        }
        // the original only set this when Player Commander was on, and
        // read it either way, so with the option off it crashed here
        let mut commander_in_a_coma = false;
        if self.camp().options.permadeath {
            let player = defs::strings(engine("PLAYER_POSITIONS"));
            if let Some(i) = self.camp().hospital.iter().position(|c| player.contains(&c.normal_position)) {
                commander_in_a_coma = true;
                if self.commander_in_a_coma(i) {
                    self.do_end();
                    return true;
                }
            }
            if commander_in_a_coma {
                self.campm().hospital.clear();
            }
        }
        if !commander_in_a_coma {
            if !self.camp().pu().alive {
                self.replace_player_tank();
            } else {
                let mut repaired = Vec::new();
                for w in &mut self.campm().pum().weapons {
                    if w.broken {
                        repaired.push(format!("Your {} is repaired.", w.name()));
                        w.broken = false;
                    }
                }
                for m in repaired {
                    self.show_message(&m, false, None);
                }
            }
            self.proceed_to_next_day(day_index);
            self.crew_replacement_check();
        }
        if self.camp().refitting() {
            self.show_start_of_day();
            if self.show_notification(
                "Your battlegroup has been recalled for refitting. Do you want to request a new tank?",
                true,
            ) {
                self.replace_player_tank();
            }
            // the original moves on with the same day index here, which
            // lands on the refitting day again; the next Enter moves on
            // from it (kept)
            self.proceed_to_next_day(day_index);
        } else {
            self.new_campaign_day();
            self.show_start_of_day();
            self.add_journal("Start of day");
            let today = self.camp().today.clone();
            self.campm().active_journal_day = Some(today);
        }
        crate::save::save_game(self);
        false
    }

    /// `ProceedToNextDay`: the next day in the calendar after
    /// `day_index`, with birthdays, a new week, and the field hospital.
    fn proceed_to_next_day(&mut self, day_index: usize) {
        let mut rng = std::mem::replace(&mut self.rng, crate::rng::Rng::new(1));
        let camp = self.campm();
        let previous = camp.today.clone();
        camp.today = camp.combat_calendar[(day_index + 1).min(camp.combat_calendar.len() - 1)].clone();
        let today = camp.today.clone();
        for p in &mut camp.pum().positions {
            if let Some(c) = &mut p.crewman {
                if previous.as_str() < c.birthday.as_str() && c.birthday <= today {
                    c.age += 1;
                }
            }
        }
        let messages = camp.check_for_new_week(&mut rng);
        self.rng = rng;
        for m in messages {
            self.show_message(&m, false, None);
        }
        if self.camp().hospital.is_empty() {
            return;
        }
        let days_past = days_between(&previous, &today);
        let release_chance = num(engine("FIELD_HOSPITAL_RELEASE_CHANCE"));
        let mut i = self.camp().hospital.len();
        while i > 0 {
            i -= 1;
            let (mut min, mut max) = self.camp().hospital[i].field_hospital.unwrap_or((0, 0));
            if min > 0 {
                min = (min - days_past).max(0);
            }
            if max > 0 {
                max = (max - days_past).max(0);
            }
            if max == 0 {
                self.show_returning_crew_menu(i);
                continue;
            }
            if min <= 0 && percentile(&mut self.rng) <= days_past as f64 * release_chance {
                self.show_returning_crew_menu(i);
                continue;
            }
            self.campm().hospital[i].field_hospital = Some((min, max));
        }
    }

    /// `LogDayRecords`
    fn log_day_records(&mut self) {
        let Some(cd) = &self.campaign_day else { return };
        let records: Records = cd.records.clone();
        let camp = self.campm();
        let today = camp.today.clone();
        match camp.logs.iter_mut().find(|(d, _)| *d == today) {
            Some((_, r)) => *r = records,
            None => camp.logs.push((today, records)),
        }
    }

    /// `ShowStartOfDay`: the date over a sunrise, faded in.
    pub fn show_start_of_day(&mut self) {
        let camp = self.camp();
        let mut c = new_console(sw(), sh(), color::BLACK, color::WHITE);
        for y in 0..sh() {
            let f = y as f64 / sh() as f64;
            c.bg = Color((255.0 * f) as u8, (170.0 * f) as u8, 0);
            c.rect(0, y, sw(), 1, true);
        }
        let (x, y) = (sxm() - 15, sh() / 2 - 4);
        c.bg = color::BLACK;
        c.rect(x, y, 30, 7, true);
        c.fg = color::LIGHT_GREY;
        draw_frame(&mut c, x, y, 30, 7);
        c.fg = color::WHITE;
        c.print_ex(sxm(), y + 2, Align::Center, &date_text(&camp.today));
        if camp.refitting() {
            c.print_ex(sxm(), y + 3, Align::Center, "Refitting");
        } else {
            c.print_ex(sxm(), y + 3, Align::Center, text(&camp.week()["day_start"]));
            c.print_name(sxm(), y + 4, Align::Center, text(&camp.week()["week_title"]));
        }
        self.fade_in(&c);
        self.wait(95, false);
    }

    /// Fade a screen in from black, as the original does over a second.
    fn fade_in(&mut self, c: &Console) {
        for i in (5..=100).rev().step_by(5) {
            self.root = c.clone();
            self.root.darken(i as f32 * 0.01);
            self.flush();
            self.wait(5, false);
        }
        self.root = c.clone();
        self.flush();
    }

    /// `ShowEndOfDay`: injuries resolved, levels gained, the dead taken
    /// away and the injured sent to the field hospital.
    pub fn show_end_of_day(&mut self) {
        let (w, h) = (70, 28);
        let (x0, y0) = (sxm() - w / 2, sh() / 2 - h / 2);
        let mut c = new_console(sw(), sh(), color::BLACK, color::WHITE);
        for _ in 0..18 {
            let (x, y) = (self.rng.int(0, sw()), self.rng.int(0, sh() * 18 / 60));
            c.put_char_ex(x, y, 250, color::WHITE, color::BLACK);
        }
        for y in sh() * 19 / 60..sh() {
            c.bg = Color((180.0 * y as f64 / sh() as f64) as u8, 0, 0);
            c.rect(0, y, sw(), 1, true);
        }
        c.bg = color::BLACK;
        c.rect(x0, y0, w, h, true);
        c.fg = color::LIGHT_GREY;
        draw_frame(&mut c, x0, y0, w, h);
        c.bg = color::DARKER_BLUE;
        c.rect(sxm() - 6, y0 + 1, 12, 1, true);
        c.bg = color::BLACK;
        c.fg = color::LIGHTER_BLUE;
        c.print_ex(sxm(), y0 + 1, Align::Center, "End of Day");
        let (xr, xl) = (x0 + 32, x0 + 52);
        c.fg = color::RED;
        c.print_ex(xr, y0 + 3, Align::Left, "Injury/KIA");
        c.fg = color::LIGHT_BLUE;
        c.print_ex(xl, y0 + 3, Align::Left, "Level Up");
        let rows: Vec<usize> = self
            .camp()
            .pu()
            .positions
            .iter()
            .enumerate()
            .filter(|(_, p)| p.crewman.is_some())
            .map(|(i, _)| i)
            .collect();
        let mut y = y0 + 5;
        for &i in &rows {
            let p = &self.camp().pu().positions[i];
            c.fg = color::WHITE;
            c.print_ex(x0 + 3, y, Align::Left, &p.name);
            c.print_name(x0 + 3, y + 1, Align::Left, &p.crewman.as_ref().unwrap().name(NameForm::Initial));
            c.fg = color::LIGHT_GREY;
            for x in x0 + 3..x0 + w - 3 {
                c.put_char(x, y + 2, b'.' as u16);
            }
            y += 3;
        }
        c.fg = color::LIGHT_GREY;
        for y1 in y0 + 4..y - 1 {
            c.put_char(x0 + 28, y1, b'.' as u16);
            c.put_char(x0 + 46, y1, b'.' as u16);
        }
        self.fade_in(&c);

        // injuries
        let mut messages = Vec::new();
        let mut rng = std::mem::replace(&mut self.rng, crate::rng::Rng::new(1));
        for p in &mut self.campm().pum().positions {
            let name = p.name.clone();
            if let Some(cr) = &mut p.crewman {
                messages.extend(cr.resolve_injuries(&name, &mut rng));
            }
        }
        self.rng = rng;
        for m in messages {
            self.show_message(&m, false, None);
        }
        crate::save::save_game(self);

        let mut y = y0 + 5;
        for &i in &rows {
            let cr = self.camp().pu().positions[i].crewman.clone().unwrap();
            if !cr.alive {
                c.fg = color::DARK_GREY;
                c.print_ex(xr + 1, y + 1, Align::Left, "KIA");
            } else if let Some((min, max)) = cr.field_hospital {
                c.fg = color::DARK_RED;
                c.print_ex(xr + 5, y, Align::Center, "Field Hospital");
                c.print_ex(xr + 5, y + 1, Align::Center, &format!("{min}-{max} days"));
            } else {
                c.fg = color::LIGHT_GREY;
                c.print_ex(xr + 5, y, Align::Center, "None");
            }
            self.root = c.clone();
            self.flush();
            self.wait(30, false);

            let extra = self.rng.int(0, 5);
            let cr = self.campm().pum().positions[i].crewman.as_mut().unwrap();
            if !cr.alive {
                c.fg = color::DARK_GREY;
                c.print_ex(xl - 1, y + 1, Align::Left, "N/A");
            } else {
                cr.fatigue = -5;
                cr.exp += extra;
                let mut levels_up = 0;
                for level in cr.level + 1..31 {
                    if cr.exp >= exp_required_for(level) {
                        levels_up += 1;
                    } else {
                        break;
                    }
                }
                let t = if levels_up == 0 {
                    c.fg = color::LIGHT_GREY;
                    "None".to_string()
                } else {
                    cr.level += levels_up;
                    cr.adv += levels_up;
                    c.fg = color::WHITE;
                    format!("+{levels_up}")
                };
                c.print_ex(xl + 4, y, Align::Center, &t);
                cr.condition = "Good Order".into();
            }
            self.root = c.clone();
            self.flush();
            self.wait(30, false);
            y += 3;
        }

        // the dead taken away, the injured to the field hospital
        let mut taken = Vec::new();
        for p in &mut self.campm().pum().positions {
            if p.crewman.as_ref().is_some_and(|c| !c.alive) {
                taken.push(format!("The body of your {} is taken away.", p.crewman.take().unwrap().normal_position));
            }
        }
        for m in taken {
            self.show_message(&m, false, None);
        }
        let camp = self.campm();
        let mut to_hospital = Vec::new();
        for p in &mut camp.pum().positions {
            if p.crewman.as_ref().is_some_and(|c| c.field_hospital.is_some()) {
                to_hospital.push(p.crewman.take().unwrap());
            }
        }
        camp.hospital.extend(to_hospital);
        camp.pum().immobilized = false;

        self.root = c;
        key_line(&mut self.root, sxm() - 7, sxm(), y0 + h - 2, "Enter", "Continue", color::LIGHT_GREY);
        self.wait_for_continue(false);
    }

    /// `CommanderInAComa`: the player commander is in the field hospital,
    /// so the calendar jumps ahead and a new tank and crew are found;
    /// true if the campaign ends first.
    fn commander_in_a_coma(&mut self, index: usize) -> bool {
        let camp = self.campm();
        for p in &mut camp.pum().positions {
            p.crewman = None;
        }
        let (min, max) = camp.hospital[index].field_hospital.unwrap_or((0, 0));
        let mut days = min;
        for _ in 0..(max - min).max(0) {
            if percentile(&mut self.rng) <= num(engine("FIELD_HOSPITAL_RELEASE_CHANCE")) {
                break;
            }
            days += 1;
        }
        // the original rolls the stay and then releases the commander ten
        // days on whatever it rolled; and it walks the calendar without
        // moving the date, so the week stays as it was and the day it
        // lands on is the combat day before the first one past the
        // release (kept, as it changes the campaign; listed in CLAUDE.md)
        let camp = self.campm();
        let mut release = camp.today.clone();
        for _ in 0..10 {
            release = crate::campaign::next_day(&release);
        }
        if release.as_str() > text(&camp.stats["end_date"]) {
            self.show_message(
                "While you are still recovering in the field hospital, you receive word that your campaign has ended.",
                false,
                None,
            );
            return true;
        }
        let mut i = camp.combat_calendar.iter().rposition(|d| *d == camp.today).unwrap_or(0);
        let mut previous = camp.today.clone();
        let mut found = false;
        let days_after: Vec<String> = camp.combat_calendar[i + 1..].to_vec();
        let mut rng = std::mem::replace(&mut self.rng, crate::rng::Rng::new(1));
        for combat_day in days_after {
            let camp = self.campm();
            let _ = camp.check_for_new_week(&mut rng);
            let today = camp.today.clone();
            let cr = &mut camp.hospital[index];
            if previous < cr.birthday && cr.birthday <= today {
                cr.age += 1;
            }
            if camp.refitting() {
                continue;
            }
            if combat_day >= release {
                camp.today = camp.combat_calendar[i].clone();
                found = true;
                break;
            }
            i += 1;
            previous = camp.combat_calendar[i].clone();
        }
        self.rng = rng;
        if !found {
            self.show_message(
                "While you are still recovering in the field hospital, you receive word that your campaign has ended.",
                false,
                None,
            );
            return true;
        }
        let msg = format!(
            "After {days} days in the field hospital, you recover and return to active duty on {}",
            date_text(&self.camp().today)
        );
        self.show_message(&msg, false, None);
        let mut cr = self.campm().hospital.remove(index);
        cr.field_hospital = None;

        let (unit_id, tank_name) = self.tank_selection_menu(false).unwrap_or_else(|| (self.camp().pu().unit_id.clone(), String::new()));
        let Some(mut unit) = Unit::new(&unit_id, &mut self.rng) else { return true };
        unit.unit_name = tank_name;
        unit.nation = text(&self.camp().stats["player_nation"]).to_string();
        let player = defs::strings(engine("PLAYER_POSITIONS"));
        if let Some(p) = unit.positions.iter_mut().find(|p| player.contains(&p.name)) {
            p.crewman = Some(cr);
            p.update_ce();
        }
        let skills = self.camp().national_skills();
        let today = self.camp().today.clone();
        unit.generate_new_personnel(&CrewContext { player: true, national_skills: &skills, today: &today }, &mut self.rng);
        unit.clear_gun_ammo();
        self.campm().player_unit = Some(unit);
        false
    }

    /// `ReplacePlayerTank`: choose a new tank and move the crew into it.
    pub fn replace_player_tank(&mut self) {
        for p in &mut self.campm().pum().positions {
            if p.crewman.as_ref().is_some_and(|c| !c.alive) {
                p.crewman = None;
            }
        }
        let transfer = defs::engine("POSITION_TRANSFER_LIST").as_object().expect("POSITION_TRANSFER_LIST");
        let (unit_id, tank_name, moves) = loop {
            let Some((unit_id, tank_name)) = self.tank_selection_menu(true) else { return };
            let mut open: Vec<String> = defs::defs().unit_types[&unit_id]["crew_positions"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|p| text(&p["name"]).to_string())
                .collect();
            let mut all_dead = true;
            let mut moves: Vec<(String, String)> = Vec::new();
            let mut discard: Vec<String> = Vec::new();
            for p in &self.camp().pu().positions {
                if p.crewman.is_none() {
                    continue;
                }
                all_dead = false;
                let list = transfer.get(&p.name).map(defs::strings).unwrap_or_default();
                match list.iter().find(|n| open.contains(n)) {
                    Some(n) => {
                        open.retain(|o| o != n);
                        moves.push((p.name.clone(), n.clone()));
                    }
                    None => discard.push(p.name.clone()),
                }
            }
            let text = if !discard.is_empty() {
                let mut t = "Crewmen in the following positions will leave your tank and be reassigned: ".to_string();
                for d in &discard {
                    t += d;
                    t.push(' ');
                }
                t
            } else if all_dead {
                "Choose this tank model?".to_string()
            } else {
                "All current crew can be assigned to this new tank.".to_string()
            };
            if self.show_notification(&text, true) {
                break (unit_id, tank_name, moves);
            }
        };
        let Some(mut unit) = Unit::new(&unit_id, &mut self.rng) else { return };
        unit.unit_name = tank_name;
        unit.nation = text(&self.camp().stats["player_nation"]).to_string();
        for (from, to) in moves {
            let old = self.campm().pum();
            let Some(cr) = old.positions.iter_mut().find(|p| p.name == from).and_then(|p| p.crewman.take()) else { continue };
            if let Some(p) = unit.positions.iter_mut().find(|p| p.name == to) {
                let mut cr = cr;
                cr.normal_position = p.name.clone();
                p.crewman = Some(cr);
            }
        }
        let skills = self.camp().national_skills();
        let today = self.camp().today.clone();
        unit.generate_new_personnel(&CrewContext { player: true, national_skills: &skills, today: &today }, &mut self.rng);
        unit.clear_gun_ammo();
        log::info!("new tank: {}", unit.unit_id);
        self.campm().player_unit = Some(unit);
    }

    /// `DoEnd`: decorations, the summary, the log file, and the save
    /// erased.
    pub fn do_end(&mut self) {
        self.award_decorations();
        self.display_campaign_summary();
        self.export_log();
        let f = self.camp().filename.clone();
        crate::save::erase_game(&f);
    }

    /// `AwardDecorations`
    fn award_decorations(&mut self) {
        let camp = self.campm();
        let Some(list) = camp.stats.get("decorations_list").and_then(|d| d.as_object()) else { return };
        let mut decos: Vec<(i32, String)> = list.iter().map(|(k, v)| (k.parse().unwrap_or(0), text(v).to_string())).collect();
        decos.sort_by(|a, b| b.0.cmp(&a.0));
        if let Some((_, d)) = decos.into_iter().find(|(vp, _)| camp.player_vp >= *vp) {
            camp.decoration = d;
        }
    }

    /// `DisplayCampaignSummary`
    fn display_campaign_summary(&mut self) {
        let camp = self.camp();
        let (w, h) = (60, 26);
        let mut t = new_console(w, h, color::BLACK, color::LIGHT_GREY);
        draw_frame(&mut t, 0, 0, w, h);
        t.fg = TITLE_COL;
        t.print_ex(w / 2, 1, Align::Center, "Your Campaign is Finished");
        t.fg = PORTRAIT_BG_COL;
        let mut y = 3;
        for line in wrap(camp.name(), 56, "") {
            t.print_name(w / 2, y, Align::Center, &line);
            y += 1;
        }
        let xm = 15;
        t.fg = color::WHITE;
        if let Some(cr) = camp.pu().positions.first().and_then(|p| p.crewman.as_ref()) {
            t.print_name(xm, 6, Align::Center, &rank_name(&cr.nation, cr.rank));
            t.print_name(xm, 7, Align::Center, &cr.name(NameForm::Full));
            let outcome = if !cr.alive {
                "KIA"
            } else if cr.injury.iter().any(|(_, v)| v.as_deref() == Some("Serious")) {
                "Discharged"
            } else {
                "Transferred"
            };
            t.print_ex(xm, 9, Align::Center, outcome);
        }
        if !camp.decoration.is_empty() {
            t.print_ex(xm, 12, Align::Center, "Awarded:");
            t.fg = color::YELLOW;
            t.print_name(xm, 13, Align::Center, &camp.decoration);
        }
        t.fg = color::WHITE;
        t.print_ex(xm, 16, Align::Center, "Total VP Earned:");
        t.print_ex(xm, 17, Align::Center, &camp.player_vp.to_string());
        let mut y = 6;
        for r in defs::strings(engine("RECORD_LIST")) {
            t.print_ex(32, y, Align::Left, &format!("{r}:"));
            t.print_ex(w - 3, y, Align::Right, &camp.records.get(&r).copied().unwrap_or(0).to_string());
            y += 1;
        }
        key_line(&mut t, w / 2 - 7, w / 2, h - 2, "Enter", "Continue", color::LIGHT_GREY);
        self.root = new_console(sw(), sh(), color::BLACK, color::WHITE);
        Console::blit(&t, 0, 0, w, h, &mut self.root, sxm() - w / 2, sh() / 2 - h / 2);
        if self.scr.is_headless() {
            return;
        }
        loop {
            self.flush();
            if self.input() && matches!(self.key().vk, Vk::Escape | Vk::Enter) {
                return;
            }
        }
    }

    /// `ExportLog`: the campaign's log as a text file, in the data folder
    /// (the original wrote it beside the game).
    fn export_log(&mut self) {
        let camp = self.camp();
        let mut s = String::new();
        s += &format!("{}\n", camp.name());
        s += &format!("{} - {}\n\n", date_text(text(&camp.stats["start_date"])), date_text(&camp.today));
        let u = camp.pu();
        s += &format!("{}\n", u.unit_id);
        if !u.unit_name.is_empty() {
            s += &format!("\"{}\"\n", u.unit_name);
        }
        s += "\n";
        for p in &u.positions {
            s += &format!("{}:\n", p.name);
            let Some(cr) = &p.crewman else {
                s += "  [Empty]\n\n";
                continue;
            };
            s += &format!("  {}\n", cr.name(NameForm::Full));
            if !cr.alive {
                s += "  KIA\n";
            } else {
                s += "  Injuries:\n";
                let mut injured = false;
                for (k, v) in &cr.injury {
                    if let Some(v) = v {
                        s += &format!("  {k}: {v}\n");
                        injured = true;
                    }
                }
                if !injured {
                    s += "  None\n";
                }
            }
            s += "\n";
        }
        s += "\n";
        for (day, records) in &camp.logs {
            s += &format!("{}:\n", date_text(day));
            for r in defs::strings(engine("RECORD_LIST")) {
                s += &format!("  {r}: {}\n", records.get(&r).copied().unwrap_or(0));
            }
            s += "\n\n";
        }
        for (day, entries) in &camp.journal {
            s += &format!("{}\n\n", date_text(day));
            for (time, entry) in entries {
                s += &format!("{time} - {entry}\n");
            }
        }
        let Some(dir) = data_dir() else { return };
        let _ = std::fs::create_dir_all(&dir);
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let path = dir.join(format!("ArmCom2_Campaign_Log_{stamp}.txt"));
        match std::fs::write(&path, s) {
            Ok(()) => log::info!("campaign log: {}", path.display()),
            Err(e) => log::error!("campaign log {}: {e}", path.display()),
        }
    }

    /// `ShowReturningCrewMenu`: a crewman back from the field hospital can
    /// take a position (replacing whoever is in it) or be transferred.
    fn show_returning_crew_menu(&mut self, index: usize) {
        if self.scr.is_headless() {
            // tests: he takes the first empty position, or is transferred
            let cr = self.campm().hospital.remove(index);
            if let Some(p) = self.campm().pum().positions.iter_mut().find(|p| p.crewman.is_none()) {
                let mut cr = cr;
                cr.field_hospital = None;
                p.crewman = Some(cr);
            }
            return;
        }
        let mut selected = 0usize;
        self.draw_returning_crew(index, selected);
        loop {
            self.flush();
            if self.take_redraw() {
                self.draw_returning_crew(index, selected);
            }
            if !self.input() {
                continue;
            }
            match self.key().vk {
                Vk::Escape => {
                    if !self.show_notification("Crewman will be permanently transferred to another unit. Continue?", true) {
                        continue;
                    }
                    self.campm().hospital.remove(index);
                    return;
                }
                Vk::Enter => {
                    let empty = self.camp().pu().positions[selected].crewman.is_none();
                    let text = if empty {
                        "Crewman will return to a currently empty position. Continue?"
                    } else {
                        "Crewman will return and replace current crewman in this position. Current crewman will be permanently transferred to another unit. Continue?"
                    };
                    if !self.show_notification(text, true) {
                        continue;
                    }
                    // the original leaves his hospital stay on him, which
                    // only shows in his crewman report
                    let cr = self.campm().hospital.remove(index);
                    let p = &mut self.campm().pum().positions[selected];
                    p.crewman = Some(cr);
                    p.update_ce();
                    return;
                }
                _ => {}
            }
            let n = self.camp().pu().positions.len();
            match self.mapped_char() {
                'w' => selected = (selected + n - 1) % n,
                's' => selected = (selected + 1) % n,
                _ => continue,
            }
            self.draw_returning_crew(index, selected);
        }
    }

    fn draw_returning_crew(&mut self, index: usize, selected: usize) {
        let keys = self.enkeys('w', 's');
        let camp = self.campaign.as_ref().expect("a campaign");
        let c = &mut self.con;
        c.bg = color::BLACK;
        c.clear();
        c.fg = color::WHITE;
        draw_frame(c, 34, 0, 32, PAGE_H);
        let u = camp.pu();
        c.fg = color::LIGHTER_BLUE;
        c.print_name(36, 1, Align::Left, &u.unit_id);
        display_crew(u, c, 35, 2, 30, Some(selected));
        key_line(c, 36, 42, 26, &keys, "Select Position", color::LIGHT_GREY);
        key_line(c, 36, 42, 27, "Enter", "Swap Crewmen", color::LIGHT_GREY);
        key_line(c, 36, 42, 28, "Esc", "Cancel Return", color::LIGHT_GREY);
        c.fg = color::CYAN;
        c.print_ex(17, 1, Align::Center, "Returning Crewman");
        c.print_ex(83, 1, Align::Center, "Replaced Crewman");
        for (x, cr) in [(2, Some(&camp.hospital[index])), (68, u.positions[selected].crewman.as_ref())] {
            let Some(cr) = cr else {
                c.fg = color::WHITE;
                c.print_ex(x, 3, Align::Left, "No Crewman");
                c.print_ex(x, 4, Align::Left, "Currently in Position");
                continue;
            };
            c.fg = ACTION_KEY_COL;
            c.print_ex(x, 3, Align::Left, "Name");
            c.print_ex(x, 5, Align::Left, "Rank");
            c.print_ex(x, 7, Align::Left, "Normal");
            c.print_ex(x, 8, Align::Left, "Position");
            c.print_ex(x, 10, Align::Left, "Stats");
            c.print_ex(x, 15, Align::Left, "Skills");
            c.print_ex(x, 25, Align::Left, "Level");
            c.print_ex(x, 26, Align::Left, "Experience");
            c.print_ex(x, 27, Align::Left, "Advance Points");
            c.fg = color::DARK_GREY;
            for y in [4, 6, 9, 14] {
                c.hline(x, y, 30);
            }
            c.fg = color::WHITE;
            c.print_name(x + 9, 3, Align::Left, &cr.name(NameForm::Full));
            c.print_name(x + 9, 5, Align::Left, &rank_name(&cr.nation, cr.rank));
            c.print_ex(x + 9, 7, Align::Left, &cr.normal_position);
            draw_stats(c, x + 9, 10, cr);
            c.fg = color::WHITE;
            for (i, s) in cr.skills.iter().take(9).enumerate() {
                c.print_ex(x + 9, 15 + i as i32, Align::Left, s);
            }
            c.print_ex(x + 28, 25, Align::Right, &cr.level.to_string());
            c.print_ex(x + 28, 26, Align::Right, &cr.exp.to_string());
            c.print_ex(x + 28, 27, Align::Right, &cr.adv.to_string());
        }
        self.show_page();
    }

    /// `ShowSwapPositionMenu`: swap the crewmen in two positions.
    pub fn show_swap_position_menu(&mut self) {
        if self.scr.is_headless() && self.script.is_empty() {
            return;
        }
        if self.camp().pu().positions.len() <= 1 {
            return;
        }
        let (mut p1, mut p2) = (0usize, 1usize);
        self.draw_swap_menu(p1, p2);
        loop {
            self.flush();
            if self.take_redraw() {
                self.draw_swap_menu(p1, p2);
            }
            if !self.input() {
                continue;
            }
            match self.key().vk {
                Vk::Escape => return,
                Vk::Enter => {
                    // as in the original, the crewmen keep their CE or BU
                    // status from where they were until it is next set
                    let u = self.campm().pum();
                    let a = u.positions[p1].crewman.take();
                    let b = u.positions[p2].crewman.take();
                    u.positions[p1].crewman = b;
                    u.positions[p2].crewman = a;
                    self.draw_swap_menu(p1, p2);
                    continue;
                }
                _ => {}
            }
            let n = self.camp().pu().positions.len();
            match self.mapped_char() {
                'q' if p1 > 0 => p1 -= 1,
                'a' if p1 + 1 < p2 => p1 += 1,
                'w' if p2 > p1 + 1 => p2 -= 1,
                's' if p2 + 1 < n => p2 += 1,
                _ => continue,
            }
            self.draw_swap_menu(p1, p2);
        }
    }

    fn draw_swap_menu(&mut self, p1: usize, p2: usize) {
        let keys_qa = self.enkeys('q', 'a');
        let keys_ws = self.enkeys('w', 's');
        let camp = self.campaign.as_ref().expect("a campaign");
        let u = camp.pu();
        let c = &mut self.con;
        c.bg = color::BLACK;
        c.clear();
        c.fg = color::WHITE;
        draw_frame(c, 1, 0, 27, 13);
        display_my_info(u, c, 2, 1, false);
        key_line(c, 3, 9, 15, &keys_qa, "Select Position 1", color::LIGHT_GREY);
        key_line(c, 3, 9, 16, &keys_ws, "Select Position 2", color::LIGHT_GREY);
        key_line(c, 3, 9, 17, "Enter", "Swap Positions", color::LIGHT_GREY);
        key_line(c, 3, 9, 19, "Esc", "Finish and Exit Menu", color::LIGHT_GREY);
        c.fg = color::WHITE;
        draw_frame(c, 34, 0, 32, PAGE_H);
        display_crew(u, c, 35, 2, 30, None);
        c.fg = color::WHITE;
        c.print_ex(72, 1, Align::Left, "Normal Position");
        c.fg = color::LIGHTER_BLUE;
        for (i, p) in u.positions.iter().enumerate() {
            if let Some(cr) = &p.crewman {
                c.print_ex(72, 2 + 4 * i as i32, Align::Left, &cr.normal_position);
            }
        }
        let (y1, y2) = (2 + 4 * p1 as i32, 2 + 4 * p2 as i32);
        c.print_ex(67, y1, Align::Left, "<\u{bf}");
        for y in y1 + 1..y2 {
            c.put_char(68, y, 179);
        }
        c.print_ex(67, y2, Align::Left, "<\u{d9}");
        self.show_page();
    }

    /// `ShowCrewmanMenu`, for a crewman in the tank or the field hospital.
    pub fn show_crewman_menu(&mut self, who: CrewRef) {
        if self.scr.is_headless() && self.script.is_empty() {
            return;
        }
        let mut selected_skill = 0usize;
        let mut n_skills = self.crewman(who).skills.len();
        self.draw_crewman_menu(who, selected_skill);
        loop {
            self.flush();
            if self.take_redraw() {
                self.draw_crewman_menu(who, selected_skill);
            }
            if !self.input() {
                continue;
            }
            if self.key().vk == Vk::Escape {
                return;
            }
            let cr = self.crewman(who);
            if !cr.alive || cr.field_hospital.is_some() {
                continue;
            }
            let k = self.mapped_char();
            match k {
                '1'..='4' => {
                    let stat = defs::strings(engine("CREW_STATS"))[k as usize - '1' as usize].clone();
                    let cr = self.crewman(who);
                    if cr.stat(&stat) == 10 {
                        self.show_notification("Stat already at maximum level.", false);
                        continue;
                    }
                    if cr.adv - 1 < 0 {
                        self.show_notification("Crewman has no Advance Points remaining.", false);
                        continue;
                    }
                    let increase = if cr.stat(&stat) < 5 { 2 } else { 1 };
                    if self.show_notification(&format!("Spend an advance point and increase {stat} by {increase}?"), true) {
                        let cr = self.crewman_mut(who);
                        cr.adv -= 1;
                        *cr.stats.entry(stat).or_insert(0) += increase;
                        crate::save::save_game(self);
                    }
                }
                'w' => selected_skill = if selected_skill == 0 { n_skills } else { selected_skill - 1 },
                's' => selected_skill = if selected_skill == n_skills { 0 } else { selected_skill + 1 },
                'f' if selected_skill == n_skills => {
                    let position = self.crew_position_name(who);
                    let skill = self.show_skill_menu(who, &position);
                    if !skill.is_empty() {
                        let cr = self.crewman_mut(who);
                        cr.adv -= 1;
                        cr.skills.push(skill);
                        crate::save::save_game(self);
                        n_skills = self.crewman(who).skills.len();
                    }
                }
                _ => continue,
            }
            self.draw_crewman_menu(who, selected_skill);
        }
    }

    pub fn crewman(&self, who: CrewRef) -> &Personnel {
        match who {
            CrewRef::Position(i) => self.camp().pu().positions[i].crewman.as_ref().expect("a crewman"),
            CrewRef::Hospital(i) => &self.camp().hospital[i],
        }
    }

    pub fn crewman_mut(&mut self, who: CrewRef) -> &mut Personnel {
        match who {
            CrewRef::Position(i) => self.campm().pum().positions[i].crewman.as_mut().expect("a crewman"),
            CrewRef::Hospital(i) => &mut self.campm().hospital[i],
        }
    }

    /// The position the crewman is in now (his normal one in hospital).
    fn crew_position_name(&self, who: CrewRef) -> String {
        match who {
            CrewRef::Position(i) => self.camp().pu().positions[i].name.clone(),
            CrewRef::Hospital(i) => self.camp().hospital[i].normal_position.clone(),
        }
    }

    /// The crewman report on the 100x30 page: commands on the left, the
    /// report in the middle, the selected skill and injuries on the right.
    fn draw_crewman_menu(&mut self, who: CrewRef, selected_skill: usize) {
        let keys_ws = self.enkeys('w', 's');
        let f = self.enkey('f').to_ascii_uppercase().to_string();
        let position = self.crew_position_name(who);
        let unit_id = self.camp().pu().unit_id.clone();
        let cr = self.crewman(who).clone();
        let active = cr.alive && cr.field_hospital.is_none();
        let n_skills = cr.skills.len();
        let c = &mut self.con;
        c.bg = color::BLACK;
        c.clear();
        c.fg = color::GREY;
        draw_frame(c, 0, 0, PAGE_W, PAGE_H);
        draw_frame(c, 26, 0, 38, PAGE_H);
        for y in [4, 6, 9, 12, 17, 20] {
            c.hline(27, y, 36);
        }

        // title
        c.bg = color::DARKER_BLUE;
        c.rect(27, 1, 36, 3, false);
        c.bg = color::BLACK;
        c.fg = color::LIGHTEST_BLUE;
        c.print_ex(45, 2, Align::Center, "Crewman Report");
        c.fg = color::YELLOW;
        for y in 1..4 {
            for x in [28, 29] {
                c.put_char(x, y, 174);
                c.put_char(x + 32, y, 175);
            }
        }

        c.fg = TITLE_COL;
        c.print_ex(27, 5, Align::Left, "Name");
        c.print_ex(27, 7, Align::Left, "Age");
        c.print_ex(27, 8, Align::Left, "Rank");
        c.print_ex(27, 10, Align::Left, "Current");
        c.print_ex(27, 11, Align::Left, "Position");
        c.print_ex(27, 13, Align::Left, "Stats");
        c.print_ex(27, 18, Align::Left, "Status");
        c.print_ex(27, 19, Align::Left, "Fatigue");
        c.print_ex(27, 21, Align::Left, "Skills");

        c.fg = color::WHITE;
        c.print_name(36, 5, Align::Left, &cr.name(NameForm::Full));
        c.print_ex(36, 7, Align::Left, &cr.age.to_string());
        c.print_ex(62, 7, Align::Right, &format!("({})", cr.birthday));
        c.print_name(36, 8, Align::Left, &rank_name(&cr.nation, cr.rank));
        if cr.field_hospital.is_some() {
            c.print_ex(36, 10, Align::Left, "Field Hospital");
        } else {
            c.print_name(36, 10, Align::Left, &unit_id);
            c.print_ex(36, 11, Align::Left, &position);
        }
        c.bg = color::DARKEST_GREY;
        c.rect(36, 14, 15, 1, false);
        c.rect(36, 16, 15, 1, false);
        c.bg = color::BLACK;
        draw_stats(c, 36, 13, &cr);

        c.fg = color::WHITE;
        c.print_ex(36, 18, Align::Left, &cr.condition);
        if cr.fatigue <= 0 {
            c.fg = color::LIGHT_GREY;
            c.print_ex(36, 19, Align::Left, "-");
        } else {
            c.fg = color::LIGHT_RED;
            c.print_ex(36, 19, Align::Left, &format!("-{}%", cr.fatigue));
        }

        // the skills, scrolled to keep the selected one in view
        let rows = 8usize;
        let first = selected_skill.saturating_sub(rows - 1);
        c.fg = color::WHITE;
        let mut list: Vec<String> = cr.skills.clone();
        if active {
            list.push("[Add New Skill]".into());
        }
        for (n, s) in list.iter().enumerate().skip(first).take(rows) {
            let y = 21 + (n - first) as i32;
            if active && n == selected_skill {
                c.bg = HIGHLIGHT_MENU_COL;
                c.rect(36, y, 26, 1, false);
                c.bg = color::BLACK;
            }
            c.print_ex(36, y, Align::Left, s);
        }

        c.bg = color::DARKEST_GREY;
        c.rect(66, 11, 26, 3, false);
        c.bg = color::BLACK;
        c.fg = TITLE_COL;
        c.print_ex(66, 11, Align::Left, "Level");
        c.print_ex(66, 12, Align::Left, "Exp");
        c.print_ex(66, 13, Align::Left, "Advance Points");
        c.fg = color::WHITE;
        c.print_ex(91, 11, Align::Right, &cr.level.to_string());
        c.print_ex(91, 12, Align::Right, &cr.exp.to_string());
        c.print_ex(91, 13, Align::Right, &cr.adv.to_string());

        // the selected skill
        let desc = if !active {
            String::new()
        } else if selected_skill == n_skills {
            "Select this option to spend an advance point and add a new skill".to_string()
        } else {
            let s = &defs::defs().skills[&cr.skills[selected_skill]];
            let mut t = text(&s["desc"]).to_string();
            if s.get("national_skill").is_some() {
                t += " (National Skill)";
            }
            t
        };
        c.fg = color::LIGHT_GREY;
        for (i, line) in wrap(&desc, 32, "").iter().take(8).enumerate() {
            c.print_ex(66, 1 + i as i32, Align::Left, line);
        }

        // injuries
        if cr.alive {
            c.bg = color::DARKER_BLUE;
            c.rect(65, 16, 34, 1, false);
            c.bg = color::BLACK;
            c.fg = TITLE_COL;
            c.print_ex(82, 16, Align::Center, "Injuries");
            for (i, (k, v)) in cr.injury.iter().enumerate() {
                let y = 18 + i as i32 * 2;
                c.fg = color::WHITE;
                c.print_ex(66, y, Align::Left, k);
                if let Some(v) = v {
                    c.fg = color::LIGHT_RED;
                    c.print_ex(97, y, Align::Right, v);
                }
            }
        }

        // commands
        if active {
            for (i, (k, ch, col)) in [("1", 4u16, color::YELLOW), ("2", 3, color::RED), ("3", 5, color::BLUE), ("4", 6, color::GREEN)]
                .into_iter()
                .enumerate()
            {
                let y = 3 + i as i32;
                key_line(c, 2, 6, y, k, "Increase", color::LIGHT_GREY);
                c.put_char_ex(15, y, ch, col, color::BLACK);
            }
            key_line(c, 2, 6, 8, &keys_ws, "Select Skill", color::LIGHT_GREY);
            if selected_skill == n_skills {
                key_line(c, 2, 6, 9, &f, "Add New Skill", color::LIGHT_GREY);
            }
        }
        key_line(c, 2, 6, 10, "Esc", "Exit Menu", color::LIGHT_GREY);
        self.show_page();
    }

    /// `ShowSkillMenu`: a skill to add, or "" if none.
    fn show_skill_menu(&mut self, who: CrewRef, position: &str) -> String {
        if self.scr.is_headless() && self.script.is_empty() {
            return String::new();
        }
        let cr = self.crewman(who).clone();
        let skills: Vec<String> = defs::defs()
            .skills
            .iter()
            .filter(|(k, v)| {
                v.get("national_skill").is_none()
                    && !cr.skills.contains(k)
                    && v.get("position_list").is_none_or(|l| defs::strings(l).iter().any(|p| p == position))
                    && v.get("prerequisite").is_none_or(|p| cr.skills.iter().any(|s| s == text(p)))
            })
            .map(|(k, _)| k.clone())
            .collect();
        if skills.is_empty() {
            self.show_message("No further skills available for this crewman.", false, None);
            return String::new();
        }
        let saved = self.root.clone();
        let mut darkened = self.root.clone();
        darkened.darken(0.5);
        let mut selected = 0usize;
        loop {
            self.root = darkened.clone();
            let (w, h) = (60, 26);
            let mut m = new_console(w, h, color::BLACK, color::WHITE);
            m.fg = color::GREY;
            draw_frame(&mut m, 0, 0, w, h);
            m.fg = TITLE_COL;
            m.print_ex(w / 2, 1, Align::Center, "Add Skill");
            let rows = (h - 7) as usize;
            let first = selected.saturating_sub(rows - 1);
            for (n, s) in skills.iter().enumerate().skip(first).take(rows) {
                let y = 3 + (n - first) as i32;
                if n == selected {
                    m.bg = HIGHLIGHT_MENU_COL;
                    m.rect(2, y, 26, 1, false);
                    m.bg = color::BLACK;
                    m.fg = color::LIGHT_GREY;
                    for (i, line) in wrap(text(&defs::defs().skills[s]["desc"]), 28, "").iter().enumerate() {
                        m.print_ex(30, 5 + i as i32, Align::Left, line);
                    }
                }
                m.fg = color::WHITE;
                m.print_ex(2, y, Align::Left, s);
            }
            let keys_ws = self.enkeys('w', 's');
            let f = self.enkey('f').to_ascii_uppercase().to_string();
            key_line(&mut m, 22, 29, h - 4, &keys_ws, "Select Skill", color::LIGHT_GREY);
            key_line(&mut m, 22, 29, h - 3, &f, "Add Skill", color::LIGHT_GREY);
            key_line(&mut m, 22, 29, h - 2, "Esc", "Cancel", color::LIGHT_GREY);
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
                    return String::new();
                }
                match self.mapped_char() {
                    'w' => selected = if selected == 0 { skills.len() - 1 } else { selected - 1 },
                    's' => selected = (selected + 1) % skills.len(),
                    'f' => {
                        if self.crewman(who).adv <= 0 {
                            self.show_notification("Crewman has no Advance Points remaining.", false);
                            continue;
                        }
                        let q = format!("Spend one advance point to gain the skill: {}?", skills[selected]);
                        if self.show_notification(&q, true) {
                            self.root = saved;
                            return skills[selected].clone();
                        }
                        continue;
                    }
                    _ => continue,
                }
                break;
            }
        }
    }
}

/// Which crewman a crewman menu is for.
#[derive(Clone, Copy, Debug)]
pub enum CrewRef {
    Position(usize),
    Hospital(usize),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cday::tests::new_game;

    /// The page as text, for looking at the layouts:
    /// `ARMCOM2_SHOTS=dir cargo test shots` writes one file a screen.
    fn shot(g: &Game, name: &str) {
        let Ok(dir) = std::env::var("ARMCOM2_SHOTS") else { return };
        let mut s = String::new();
        for y in 0..g.root.h {
            for x in 0..g.root.w {
                s.push(crate::glyph::to_char(g.root.char_at(x, y)));
            }
            s.push('\n');
        }
        std::fs::write(format!("{dir}/{name}.txt"), s).unwrap();
    }

    #[test]
    fn shots() {
        let mut g = new_game("battle_of_france_ger", 7);
        g.add_journal("Start of day");
        for m in 1..=4 {
            g.campm().active_calendar_menu = m;
            g.draw_calendar(1, 0);
            shot(&g, &format!("calendar_{m}"));
        }
        let guns: Vec<usize> = (0..g.camp().pu().weapons.len()).filter(|&i| g.camp().pu().weapons[i].ammo_stores.is_some()).collect();
        default_load_for_test(&mut g, guns[0]);
        g.draw_ammo_menu(&guns, &crate::cday::AmmoMenu { gun: 0, ammo_type: "AP".into(), add_num: 1, use_rr: false });
        shot(&g, "ammo");
        g.draw_crewman_menu(CrewRef::Position(0), 0);
        shot(&g, "crewman");
        g.draw_swap_menu(0, 2);
        shot(&g, "swap");
        let units: Vec<Unit> = defs::strings(&g.camp().stats["player_unit_list"]).iter().filter_map(|id| Unit::new(id, &mut g.rng)).collect();
        if !units.is_empty() {
            g.draw_tank_selection(&units, 0, "Lucky", false);
            shot(&g, "tank_selection");
        }
        let mut list: Vec<crate::campaign::CampaignInfo> = defs::defs()
            .campaigns
            .iter()
            .map(|(f, v)| crate::campaign::CampaignInfo { filename: f.clone(), data: v })
            .collect();
        list.sort_by(|a, b| text(&a.data["start_date"]).cmp(text(&b.data["start_date"])));
        g.draw_campaign_selection(&list, 3);
        shot(&g, "campaign_selection");
        let cr = g.camp().pu().positions[1].crewman.clone().unwrap();
        g.campm().hospital.push(cr);
        g.draw_returning_crew(0, 1);
        shot(&g, "returning_crew");
    }

    /// Whole campaigns from New Campaign to the end of the calendar, every
    /// day passing without action as the day map is not ported yet; more
    /// with `ARMCOM2_CAMPAIGNS=n`.
    #[test]
    fn campaigns_run_to_the_end() {
        let n: u64 = std::env::var("ARMCOM2_CAMPAIGNS").ok().and_then(|v| v.parse().ok()).unwrap_or(10);
        let from: u64 = std::env::var("ARMCOM2_FROM").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
        for seed in from..=n {
            if std::env::var("ARMCOM2_TRACE").is_ok() {
                eprintln!("seed {seed}");
            }
            let mut g = Game::new(crate::ui::Screen::headless(), crate::rng::Rng::new(seed));
            assert!(g.start_new_campaign());
            g.do_campaign_calendar_loop();
            // every day is played, unless the commander was sent to the
            // field hospital (which the original does one time in 1001
            // even without an injury) and the calendar jumped
            let camp = g.camp();
            let logged: Vec<&String> = camp.logs.iter().map(|(d, _)| d).collect();
            let missing = camp.combat_calendar.iter().filter(|d| !logged.contains(d)).count();
            let jumped = camp.journal.is_empty() || missing > 0;
            assert!(camp.today == *camp.combat_calendar.last().unwrap() || jumped, "seed {seed}: {}", camp.filename);
        }
    }

    #[test]
    fn scripted_crew_screens() {
        let mut g = new_game("poland_campaign", 5);
        // crewman menu: raise Perception, try to add a skill, leave
        g.script.extend(['1', 'y', 's', 'f', '\u{1b}', '\u{1b}'].map(crate::game::press));
        let before = g.crewman(CrewRef::Position(0)).clone();
        g.show_crewman_menu(CrewRef::Position(0));
        let after = g.crewman(CrewRef::Position(0));
        assert_eq!(after.stat("Perception"), before.stat("Perception") + 2);
        assert_eq!(after.adv, before.adv - 1);
        // swap the first two positions
        let a = g.camp().pu().positions[0].crewman.as_ref().unwrap().last_name.clone();
        g.script.extend(['\r', '\u{1b}'].map(crate::game::press));
        g.show_swap_position_menu();
        assert_eq!(g.camp().pu().positions[1].crewman.as_ref().unwrap().last_name, a);
        assert!(g.script.is_empty());
    }

    fn default_load_for_test(g: &mut Game, i: usize) {
        let w = &mut g.campm().pum().weapons[i];
        w.load_gun_ammo();
    }
}
