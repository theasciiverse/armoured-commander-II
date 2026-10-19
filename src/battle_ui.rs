//! The battle on the screen (`DoScenarioLoop` and the `Scenario` console
//! functions): the hex map, the panels around it, the attack and
//! firepower windows, the bail-out, the animations, and the player's
//! commands.
//!
//! Layout: the screen is the terminal's. The hex map uses Armoured
//! Commander I's terminal hexes (`k` rows apart, 59x29 at k=4 and bigger
//! on bigger screens), with a 23-column left column (the player's tank in
//! a line, or in full from 45 rows, the crew three rows a position, the
//! commands) and an 18-column right column (date, time and phase, the
//! context panel, weather, the unit under the pointer); both widen a
//! little on wider screens. The attack window, which replaced the
//! original's left column, is a two-column window over the map.

use crate::battle::*;
use crate::calendar::CrewRef;
use crate::campaign::display_my_info;
use crate::color::{self, Color};
use crate::console::{Align, Console};
use crate::defs::{self, engine, int, text};
use crate::game::*;
use crate::hexes::{distance, get_line};
use crate::layout::*;
use crate::model::NameForm;
use crate::ui::{Vk, wrap};
use std::time::{Duration, Instant};

const ANIM_EVERY: Duration = Duration::from_millis(150);
const ENEMY_UNIT_COL: Color = Color(255, 20, 20);
const ALLIED_UNIT_COL: Color = Color(120, 120, 255);
const UNKNOWN_UNIT_COL: Color = Color(127, 127, 127);

/// Where everything goes on the battle screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BattleLayout {
    pub left_w: i32,
    pub right_w: i32,
    pub map_x: i32,
    pub map_y: i32,
    pub map_w: i32,
    pub map_h: i32,
    pub k: i32,
}

fn map_size(k: i32) -> (i32, i32) {
    let dx = 3 * k / 2 + 2;
    (6 * dx + 2 * k + 3, 7 * k + 1)
}

impl BattleLayout {
    pub fn new() -> Self {
        Self::for_screen(sw(), sh())
    }

    pub fn for_screen(w: i32, h: i32) -> Self {
        let mut k = 4;
        for kk in [8, 6] {
            let (mw, mh) = map_size(kk);
            if mw + 41 <= w && mh <= h {
                k = kk;
                break;
            }
        }
        let (map_w, map_h) = map_size(k);
        let extra = (w - 41 - map_w).max(0);
        let left_w = 23 + (extra / 2).min(3);
        let right_w = 18 + (extra - (left_w - 23)).min(4);
        let map_x = left_w + (w - left_w - right_w - map_w).max(0) / 2;
        let map_y = (h - map_h).max(0) / 2;
        BattleLayout { left_w, right_w, map_x, map_y, map_w, map_h, k }
    }

    pub fn dx(&self) -> i32 {
        3 * self.k / 2 + 2
    }

    /// A hex's centre on the screen (`PlotHex`, for the terminal).
    pub fn plot(&self, hx: i32, hy: i32) -> (i32, i32) {
        let (x0, y0) = (self.map_x + self.map_w / 2, self.map_y + self.map_h / 2);
        (x0 + hx * self.dx(), y0 + hy * self.k + hx * self.k / 2)
    }

    /// A hex's rows as (dx, dy, width) from its centre: `2k+2` wide on top,
    /// as Armoured Commander I's.
    pub fn hex_rows(&self) -> Vec<(i32, i32, i32)> {
        let half = self.k / 2;
        let mut rows = vec![(-half, -half, 2 * half + 2)];
        for r in -half + 1..=half {
            let left = if r <= 0 { -2 * half - r } else { -2 * half + r - 1 };
            rows.push((left, r, 1 - 2 * left));
        }
        rows
    }

    /// The hex under a screen cell, if any of the 37 on the map.
    pub fn hex_at(&self, x: i32, y: i32) -> Option<(i32, i32)> {
        let rows = self.hex_rows();
        let mut found = None;
        for r in 0..4 {
            for h in crate::hexes::ring(0, 0, r) {
                let (cx, cy) = self.plot(h.0, h.1);
                for &(dx, dy, w) in &rows {
                    if y == cy + dy && x > cx + dx && x < cx + dx + w - 1 {
                        found = Some(h);
                    }
                }
            }
        }
        found
    }
}

/// The battle's animations (`Scenario.animation`), and what the windows
/// over the map show.
#[derive(Default)]
pub struct BattleAnim {
    pub rain: Vec<(i32, i32, i32)>,
    pub snow: Vec<(i32, i32, i32)>,
    pub gun_line: Vec<(i32, i32)>,
    pub small_arms: Option<(Vec<(i32, i32)>, i32)>,
    pub bomb: Option<((i32, i32), i32)>,
    pub grenade: Option<((i32, i32), i32)>,
    pub flame: Option<((i32, i32), i32)>,
    pub air: Option<(i32, Vec<(i32, i32)>)>,
    pub highlight: Option<(i32, i32)>,
    pub flash: bool,
    pub last: Option<Instant>,
    /// Units moving across the map: (unit, cells), first cell current.
    pub moves: Vec<(usize, Vec<(i32, i32)>)>,
    /// The attack window, if up.
    pub attack: Option<AttackView>,
    /// The firepower window, if up.
    pub fp_window: Option<Console>,
    /// The battle screen is up (so waits keep it moving).
    pub active: bool,
    pub mouse_seen: bool,
}

pub struct AttackView {
    pub profile: Profile,
    /// Waiting for the player before the roll.
    pub prompt: bool,
    pub roll: Option<f64>,
    pub rof: Option<bool>,
    pub done: bool,
}

impl Game {
    /// `ShowMessage` in a battle: near the hex it is about, if any; the
    /// hex's outline flashes.
    pub fn battle_message(&mut self, text: &str, longer: bool, portrait: Option<&str>, hex: Option<(i32, i32)>) {
        if self.scenario.is_none() || !self.battle_anim.active {
            self.show_message(text, longer, portrait);
            return;
        }
        let l = BattleLayout::new();
        let near = hex.map(|(hx, hy)| {
            let (x, y) = l.plot(hx, hy);
            (x, y + if y < sh() / 2 { l.k / 2 } else { -l.k / 2 }, y < sh() / 2)
        });
        self.battle_anim.highlight = hex;
        self.show_message_at(text, longer, portrait, near);
        self.battle_anim.highlight = None;
        self.draw_battle();
    }

    // ---------------------------------------------------------------------
    // animations

    /// `InitAnimations`
    fn init_battle_animations(&mut self) {
        let l = BattleLayout::new();
        let precip = self.cd().weather.precipitation.clone();
        let mut a = std::mem::take(&mut self.battle_anim);
        a.rain.clear();
        a.snow.clear();
        a.highlight = None;
        let area = |g: &mut Game| (g.rng.int(l.map_x, l.map_x + l.map_w - 1), g.rng.int(l.map_y, l.map_y + l.map_h - 1));
        match precip.as_str() {
            "Rain" | "Heavy Rain" => {
                for _ in 0..if precip == "Heavy Rain" { 8 } else { 4 } * l.k / 4 {
                    let (x, y) = area(self);
                    a.rain.push((x, y, 4));
                }
            }
            "Light Snow" | "Snow" | "Blizzard" => {
                let n = match precip.as_str() {
                    "Light Snow" => 4,
                    "Snow" => 8,
                    _ => 16,
                } * l.k
                    / 4;
                for _ in 0..n {
                    let (x, y) = area(self);
                    let life = self.rng.int(4, 10);
                    a.snow.push((x, y, life));
                }
            }
            _ => {}
        }
        self.battle_anim = a;
    }

    /// `UpdateAnimCon`: one step of every animation.
    fn step_battle_animations(&mut self) {
        let l = BattleLayout::new();
        let mut a = std::mem::take(&mut self.battle_anim);
        for d in &mut a.rain {
            if d.2 == 0 {
                *d = (self.rng.int(l.map_x, l.map_x + l.map_w - 1), self.rng.int(l.map_y, l.map_y + l.map_h - 1), self.rng.int(1, 5));
            } else {
                d.1 += 1;
                d.2 -= 1;
            }
        }
        for f in &mut a.snow {
            if f.2 == 0 {
                *f = (self.rng.int(l.map_x, l.map_x + l.map_w - 1), self.rng.int(l.map_y, l.map_y + l.map_h - 1), self.rng.int(4, 10));
            } else {
                f.0 += self.rng.int(-1, 1);
                f.1 += 1;
                f.2 -= 1;
            }
        }
        if let Some((_, line)) = &mut a.air {
            if !line.is_empty() {
                line.remove(0);
            }
            if line.is_empty() {
                a.air = None;
            }
        }
        if !a.gun_line.is_empty() {
            a.gun_line.remove(0);
            if !a.gun_line.is_empty() {
                a.gun_line.remove(0);
            }
        }
        for slot in [&mut a.bomb, &mut a.grenade, &mut a.flame] {
            if let Some((_, life)) = slot {
                if *life == 0 {
                    *slot = None;
                } else {
                    *life -= 1;
                }
            }
        }
        if let Some((_, life)) = &mut a.small_arms {
            if *life == 0 {
                a.small_arms = None;
            } else {
                *life -= 1;
            }
        }
        a.flash = !a.flash;
        a.last = Some(Instant::now());
        self.battle_anim = a;
    }

    /// Keep the battle screen moving while something waits.
    pub fn battle_tick(&mut self) {
        if !self.battle_anim.active || self.scenario.is_none() {
            return;
        }
        if self.battle_anim.last.is_none_or(|t| t.elapsed() >= ANIM_EVERY) {
            self.step_battle_animations();
            self.draw_battle();
        }
    }

    /// Run the screen until the given animation is over.
    fn run_animation(&mut self, done: impl Fn(&BattleAnim) -> bool) {
        if self.scr.is_headless() {
            let a = &mut self.battle_anim;
            a.gun_line.clear();
            a.small_arms = None;
            a.bomb = None;
            a.grenade = None;
            a.flame = None;
            a.air = None;
            return;
        }
        let start = Instant::now();
        while !done(&self.battle_anim) && start.elapsed() < Duration::from_secs(10) {
            self.battle_tick();
            self.flush();
            self.input();
        }
    }

    /// The attack's animation: a shell's flight and explosion, a burst of
    /// small arms fire, grenades, a flame thrower.
    pub fn animate_attack(&mut self, att: usize, w: usize, tgt: usize) {
        let l = BattleLayout::new();
        let (a, t) = (self.su(att), self.su(tgt));
        let (x1, y1) = l.plot(a.hx, a.hy);
        let (x2, y2) = l.plot(t.hx, t.hy);
        let weapon = a.weapons[w].clone();
        let wp = &weapon;
        let wt = wp.stat("type").unwrap_or("").to_string();
        let mg = defs::strings(engine("MG_WEAPONS"));
        crate::sound::play_fire(wp, &mut self.rng);
        if wt == "Gun" {
            let he = wp.ammo_type.as_deref() == Some("HE");
            self.battle_anim.gun_line = get_line(x1, y1, x2, y2, false);
            self.run_animation(|a| a.gun_line.is_empty());
            if he {
                crate::sound::play_he_explosion(&mut self.rng);
                self.battle_anim.bomb = Some(((x2, y2), 7));
                self.run_animation(|a| a.bomb.is_none());
            }
        } else if wt == "Small Arms" || mg.contains(&wt) {
            self.battle_anim.small_arms = Some((get_line(x1, y1, x2, y2, false), 12));
            self.run_animation(|a| a.small_arms.is_none());
        } else if wt == "Close Combat" {
            if wp.name() == "Grenades" {
                self.battle_anim.grenade = Some(((x2, y2), 3));
                self.run_animation(|a| a.grenade.is_none());
            } else if wp.name() == "Flame Thrower" {
                self.battle_anim.flame = Some(((x2, y2), 9));
                self.run_animation(|a| a.flame.is_none());
            }
        }
    }

    /// Units sliding to their new hexes as the player moves.
    pub fn animate_unit_moves(&mut self, moves: &[(usize, (i32, i32), (i32, i32))]) {
        let l = BattleLayout::new();
        let mut paths = Vec::new();
        for &(u, from, to) in moves {
            let (x1, y1) = l.plot(from.0, from.1);
            let (x2, y2) = l.plot(to.0, to.1);
            paths.push((u, get_line(x1, y1, x2, y2, false)));
        }
        for step in 0..6 {
            self.battle_anim.moves = paths
                .iter()
                .map(|(u, line)| {
                    let at = (line.len() - 1) * (step + 1) / 6;
                    (*u, vec![line[at]])
                })
                .collect();
            self.draw_battle();
            self.flush();
            self.wait(15, false);
        }
        self.battle_anim.moves.clear();
    }

    /// An air attack diving on a hex; the direction it came from.
    pub fn animate_air_attack(&mut self, h: (i32, i32)) -> i32 {
        let l = BattleLayout::new();
        let (x, y) = l.plot(h.0, h.1);
        // the original's direction comes from its square-cell plot
        let (_, ys) = plot_sq(h.0, h.1);
        let direction = if ys <= 30 { -1 } else { 1 };
        let span = l.k * 2;
        let (y1, y2) = if direction == -1 {
            ((y + span).min(l.map_y + l.map_h - 1), y + 1)
        } else {
            ((y - span).max(l.map_y), y - 1)
        };
        crate::sound::play("plane_incoming_00");
        self.battle_anim.air = Some((direction, get_line(x, y1, x, y2, false)));
        self.run_animation(|a| a.air.is_none());
        crate::sound::play("stuka_divebomb_00");
        self.battle_anim.bomb = Some(((x, y2 + direction), 14));
        self.run_animation(|a| a.bomb.is_none());
        direction
    }

    /// Shells landing around a hex.
    pub fn animate_bombardment(&mut self, h: (i32, i32)) {
        let l = BattleLayout::new();
        let (x, y) = l.plot(h.0, h.1);
        for _ in 0..3 {
            let (xm, ym) = (3 - self.rng.int(0, 6), (3 - self.rng.int(0, 6)) / 2);
            crate::sound::play_he_explosion(&mut self.rng);
            self.battle_anim.bomb = Some(((x + xm, y + ym), 3));
            self.run_animation(|a| a.bomb.is_none());
        }
    }

    // ---------------------------------------------------------------------
    // the attack and firepower windows

    /// `DisplayAttack`; `prompt` while waiting for the player to go on.
    pub fn show_attack(&mut self, p: &Profile, prompt: bool) {
        if self.scr.is_headless() && self.script.is_empty() && !self.render_headless {
            return;
        }
        let roll = if prompt { None } else { Some(p.roll) };
        self.battle_anim.attack = Some(AttackView { profile: p.clone(), prompt, roll, rof: None, done: !prompt });
        self.draw_battle();
        self.flush();
    }

    /// The rolling number while the dice roll.
    pub fn attack_roll_shown(&mut self, roll: Option<f64>) {
        if let Some(v) = &mut self.battle_anim.attack {
            v.roll = roll;
            v.prompt = false;
        }
        self.draw_battle();
        self.flush();
    }

    pub fn show_attack_rof(&mut self, kept: bool) {
        if let Some(v) = &mut self.battle_anim.attack {
            v.rof = Some(kept);
        }
    }

    /// Wait for Enter after an attack, or F to fire again if the rate of
    /// fire was kept: `Some(true)` to fire again.
    pub fn wait_attack_result(&mut self, can_fire_again: bool) -> Option<bool> {
        if self.scr.is_headless() && self.script.is_empty() {
            return Some(false);
        }
        self.draw_battle();
        loop {
            self.battle_tick();
            self.flush();
            if !self.input() {
                continue;
            }
            if self.key().vk == Vk::Enter {
                return Some(false);
            }
            if can_fire_again && self.mapped_char() == 'f' {
                return Some(true);
            }
        }
    }

    pub fn close_attack(&mut self) {
        self.battle_anim.attack = None;
        if self.battle_anim.active {
            self.draw_battle();
        }
    }

    /// The attack window: who attacks whom and with what on the left, the
    /// chance and its modifiers on the right.
    fn draw_attack_window(&self, root: &mut Console) {
        let Some(v) = &self.battle_anim.attack else { return };
        let p = &v.profile;
        let s = self.sc();
        let (a, t) = (&s.units[p.attacker], &s.units[p.target]);
        let (w, h) = (54, 26);
        let l = BattleLayout::new();
        let x0 = (l.map_x + l.map_w / 2 - w / 2).clamp(0, sw() - w);
        let y0 = (sh() / 2 - h / 2).max(0);
        let mut c = new_console(w, h, color::BLACK, color::WHITE);
        draw_frame(&mut c, 0, 0, w, h);
        let attacker_spotted = !(a.owning_player == 1 && !a.spotted);
        let target_spotted = !(t.owning_player == 1 && !t.spotted);
        let title = match p.kind.as_str() {
            "ap" => "Armour Penetration",
            "Close Combat" => "Close Combat",
            _ => "Ranged Attack",
        };
        c.bg = color::DARKER_BLUE;
        c.rect(1, 1, w - 2, 1, false);
        c.bg = color::BLACK;
        c.print_ex(w / 2, 1, Align::Center, title);

        // left column
        let portrait = |c: &mut Console, u: &crate::model::Unit, spotted: bool, y: i32| {
            c.bg = PORTRAIT_BG_COL;
            c.rect(1, y, 25, 4, false);
            let name = if spotted { u.stat("portrait") } else { Some("unit_unknown.xp") };
            if let Some(img) = name.and_then(crate::xp::by_name) {
                crate::art::draw(&img, c, 1, y, 24, 4);
            }
            c.bg = color::BLACK;
        };
        let mut y = 3;
        if p.kind == "Point Fire" || p.kind == "Area Fire" {
            portrait(&mut c, a, attacker_spotted, y);
        }
        y += 5;
        let wname = a.weapons.get(p.weapon).map(|w| w.name().to_string()).unwrap_or_default();
        let (t1, t2, t3) = if p.kind == "ap" {
            let mut t2 = if attacker_spotted { format!("hit by {wname}") } else { "hit".into() };
            if let Some(am) = &p.ammo_type {
                t2 += &format!(" ({am})");
            }
            (t.name().to_string(), t2, format!("in {}", p.location_desc))
        } else {
            let mut t2 = if attacker_spotted { format!("attacks w/ {wname}") } else { "attacks".into() };
            if attacker_spotted {
                if let Some(am) = &a.weapons[p.weapon].ammo_type {
                    t2 += &format!(" {am}");
                }
            }
            (a.name().to_string(), t2, t.name().to_string())
        };
        for (n, line) in [t1, t2, t3].iter().enumerate() {
            c.print_name(13, y + n as i32, Align::Center, line);
        }
        portrait(&mut c, t, target_spotted, y + 4);

        // right column
        let x = 28;
        let xm = x + 12;
        let base_label = match p.kind.as_str() {
            "ap" => "Base Score to Penetrate",
            "Area Fire" => "Base Chance of Effect",
            _ => "Base Chance to Hit",
        };
        c.print_ex(xm, 3, Align::Center, base_label);
        let base = if p.kind == "ap" { format!("{}", p.base_chance as i32) } else { format!("{}%", py_f(p.base_chance)) };
        c.print_ex(xm, 4, Align::Center, &base);
        c.bg = color::DARKER_BLUE;
        c.rect(x, 6, 24, 1, false);
        c.bg = color::BLACK;
        c.print_ex(xm, 6, Align::Center, "Modifiers");
        let mut my = 7;
        if p.modifier_list.is_empty() {
            c.print_ex(xm, my, Align::Center, "None");
        }
        for (desc, m) in p.modifier_list.iter().take(9) {
            let d: String = desc.chars().take(17).collect();
            c.fg = color::WHITE;
            c.print_ex(x, my, Align::Left, &d);
            let (col, sign) = if p.kind == "ap" {
                if *m < 0.0 { (color::RED, "") } else { (color::GREEN, "+") }
            } else if *m > 0.0 {
                (color::GREEN, "+")
            } else {
                (color::RED, "")
            };
            c.fg = col;
            let val = if p.kind == "ap" { format!("{sign}{}", *m as i32) } else { format!("{sign}{}", py_f(*m)) };
            c.print_ex(x + 23, my, Align::Right, &val);
            my += 1;
        }
        c.fg = color::WHITE;
        if p.kind == "ap" {
            c.print_ex(xm, 16, Align::Center, &format!("Score required: {}", p.final_score));
        }
        c.bg = color::DARKER_BLUE;
        c.rect(x, 17, 24, 1, false);
        c.bg = color::BLACK;
        c.print_ex(xm, 17, Align::Center, "Final Chance");
        let bar = |c: &mut Console, chance: f64, col: Color| {
            let n = (24.0 * chance / 100.0).ceil() as i32;
            c.bg = col;
            c.rect(x, 18, n.min(24), 2, false);
        };
        if p.kind == "Area Fire" {
            bar(&mut c, 100.0, color::RED);
            bar(&mut c, p.final_chance, color::DARKER_GREEN);
            bar(&mut c, p.full_effect, color::GREEN);
            bar(&mut c, p.critical_effect, color::BLUE);
            c.bg = color::BLACK;
            c.fg = color::WHITE;
            if p.final_chance > p.full_effect {
                c.print_ex(x + 23, 18, Align::Right, &format!("PART {}%", py_f(p.final_chance)));
            }
            if p.full_effect > p.critical_effect {
                c.print_ex(xm, 19, Align::Center, &format!("FULL {}%", py_f(p.full_effect)));
            }
            c.print_ex(x, 18, Align::Left, &format!("CRIT {}%", py_f(p.critical_effect)));
        } else {
            bar(&mut c, 100.0, color::RED);
            bar(&mut c, p.final_chance, color::GREEN);
            if p.kind != "ap" {
                for yy in 18..20 {
                    c.put_char_ex(x, yy, 221, color::BLUE, c.char_bg(x, yy));
                    c.put_char_ex(x + 23, yy, 222, color::DARK_GREY, c.char_bg(x + 23, yy));
                }
            }
            c.bg = color::BLACK;
            c.fg = color::WHITE;
            let mut t = format!("{}%", py_f(p.final_chance));
            if p.kind == "ap" {
                if p.final_chance == 0.0 {
                    t = "Impossible".into();
                } else if p.final_chance == 100.0 {
                    t = "Automatic".into();
                }
            }
            c.print_ex(xm, 18, Align::Center, &t);
        }
        if let Some(r) = v.roll {
            c.print_ex(xm, 20, Align::Center, &format!("{}%", py_f(r)));
        }
        if v.done {
            c.print_ex(xm, 21, Align::Center, &p.result);
            if p.kind == "Area Fire" && p.result != "NO EFFECT" {
                c.print_ex(xm, 22, Align::Center, &format!("{} FP", p.effective_fp));
            }
        }
        let f = self.enkey('f').to_ascii_uppercase().to_string();
        if v.rof == Some(true) {
            c.print_ex(xm, 23, Align::Center, "Maintained Rate of Fire");
            key_line(&mut c, 3, 9, h - 3, &f, "Fire Again", color::WHITE);
        }
        if v.prompt {
            if p.attacker == s.player && p.kind != "ap" {
                key_line(&mut c, 3, 9, h - 3, "Bksp", "Cancel", color::WHITE);
            }
            key_line(&mut c, 3, 9, h - 2, "Enter", "Continue", color::WHITE);
        } else if v.done {
            key_line(&mut c, 3, 9, h - 2, "Enter", "Continue", color::WHITE);
        }
        Console::blit(&c, 0, 0, w, h, root, x0, y0);
    }

    /// The firepower window (`ResolveFP`): the odds, then the roll and
    /// the outcome; Enter skips the pause.
    pub fn show_fp_window(&mut self, i: usize, fp: i32, destroy: f64, reduction: f64, rout: f64, result: Option<(f64, String)>) {
        let (w, h) = (26, 17);
        let mut c = new_console(w, h, color::BLACK, color::WHITE);
        draw_frame(&mut c, 0, 0, w, h);
        c.bg = color::DARKER_BLUE;
        c.rect(1, 1, w - 2, 2, false);
        c.bg = color::BLACK;
        c.print_ex(13, 1, Align::Center, &format!("Resolving {fp} firepower"));
        c.print_name(13, 2, Align::Center, &format!("on {}", self.uname(i)));
        let u = self.su(i);
        let name = if !u.spotted && u.owning_player == 1 { Some("unit_unknown.xp") } else { u.stat("portrait") };
        c.bg = PORTRAIT_BG_COL;
        c.rect(1, 3, 24, 4, false);
        if let Some(img) = name.and_then(crate::xp::by_name) {
            crate::art::draw(&img, &mut c, 1, 3, 24, 4);
        }
        c.bg = color::BLACK;
        c.print_ex(13, 8, Align::Center, "Outcome Odds:");
        let mut y = 9;
        for (label, v) in [("Destroy:", destroy), ("Reduce:", reduction), ("Rout:", rout)] {
            if v > 0.0 {
                c.print_ex(1, y, Align::Left, label);
                c.print_ex(24, y, Align::Right, &format!("{}%", py_f(v)));
                y += 1;
            }
        }
        if let Some((roll, outcome)) = &result {
            c.print_ex(13, 13, Align::Center, &format!("Roll: {}", py_f(*roll)));
            c.print_ex(13, 14, Align::Center, outcome);
        }
        key_line(&mut c, 8, 15, h - 2, "Enter", "Skip", color::LIGHT_GREY);
        self.battle_anim.fp_window = Some(c);
        self.draw_battle();
        let pause = if result.is_some() { 400 } else { 300 } + 40 * self.settings.message_pause.max(0) as u64;
        self.wait(pause, true);
        if result.is_some() {
            self.battle_anim.fp_window = None;
            self.draw_battle();
        }
    }

    // ---------------------------------------------------------------------
    // the bail-out

    /// The bail-out screen: each crewman's knock-out wound, bail-out roll,
    /// and rescue, with the burn-up in the middle.
    pub fn draw_bail_out(&mut self, cols: &[[String; 3]], burn: &str, done: bool) {
        if self.scr.is_headless() && !self.render_headless {
            return;
        }
        let pl = self.pl();
        let u = self.su(pl).clone();
        let mut c = new_console(PAGE_W, PAGE_H, color::BLACK, color::WHITE);
        c.bg = color::LIGHT_RED;
        c.rect(0, 0, PAGE_W, 3, true);
        c.fg = color::BLACK;
        c.print_ex(PAGE_W / 2, 1, Align::Center, "BAILING OUT");
        c.bg = color::BLACK;
        let px = PAGE_W / 2 - 12;
        c.fg = color::LIGHTER_BLUE;
        c.print_name(px, 3, Align::Left, &u.unit_id);
        c.fg = color::LIGHT_GREY;
        c.print_ex(px + 24, 3, Align::Right, u.stat("class").unwrap_or(""));
        c.bg = PORTRAIT_BG_COL;
        c.rect(px, 4, 25, 4, true);
        if let Some(img) = u.stat("portrait").and_then(crate::xp::by_name) {
            crate::art::draw(&img, &mut c, px, 4, 24, 4);
        }
        c.fg = color::WHITE;
        if !u.unit_name.is_empty() {
            c.print_name(px, 4, Align::Left, &u.unit_name);
        }
        c.bg = color::BLACK;
        let (xn, xl, x2, x3, x4) = (4, 29, 36, 54, 72);
        c.fg = color::WHITE;
        c.print_ex(x2, 10, Align::Left, "KO Wound");
        c.print_ex(x3, 10, Align::Left, "Bail Out");
        c.print_ex(x4, 10, Align::Left, "Rescue");
        if !burn.is_empty() {
            c.fg = if burn == "Tank Burns" { color::LIGHT_RED } else { color::LIGHT_GREY };
            c.print_ex(x4, 9, Align::Left, burn);
        }
        let mut y = 11;
        for (n, p) in u.positions.iter().enumerate() {
            c.fg = color::LIGHT_BLUE;
            c.print_ex(xn, y, Align::Left, &p.name);
            c.fg = color::WHITE;
            c.print_ex(xl, y, Align::Right, p.location.as_deref().unwrap_or(""));
            match &p.crewman {
                None => c.print_ex(xn, y + 1, Align::Left, "Empty"),
                Some(cr) => {
                    c.print_name(xn, y + 1, Align::Left, &cr.name(NameForm::Initial));
                    c.print_ex(xl, y + 1, Align::Right, if cr.ce { "CE" } else { "BU" });
                    let (col, t) = if !cr.alive {
                        (color::DARK_GREY, "Dead")
                    } else if cr.condition == "Stunned" {
                        (color::LIGHT_GREY, "Stunned")
                    } else if cr.condition == "Unconscious" {
                        (color::GREY, "Unconscious")
                    } else {
                        (color::WHITE, "")
                    };
                    c.fg = col;
                    c.print_ex(xl, y + 2, Align::Right, t);
                }
            }
            c.fg = color::LIGHT_GREY;
            for (k, x) in [x2, x3, x4].into_iter().enumerate() {
                for (m, line) in wrap(&cols[n][k], 16, "").iter().take(3).enumerate() {
                    c.print_ex(x, y + m as i32, Align::Left, line);
                }
            }
            c.fg = color::DARKER_GREY;
            for yy in y..y + 3 {
                c.put_char(32, yy, 250);
            }
            y += 3;
        }
        if done {
            key_line(&mut c, PAGE_W / 2 - 7, PAGE_W / 2, PAGE_H - 1, "Enter", "Continue", color::WHITE);
        }
        self.root = new_console(sw(), sh(), color::BLACK, color::WHITE);
        Console::blit(&c, 0, 0, PAGE_W, PAGE_H, &mut self.root, page_x(), page_y());
        self.flush();
    }

    // ---------------------------------------------------------------------
    // the screen

    /// `UpdateScenarioDisplay`, everything drawn again.
    pub fn draw_battle(&mut self) {
        if (self.scr.is_headless() && !self.render_headless) || self.scenario.is_none() {
            return;
        }
        let l = BattleLayout::new();
        let mut root = new_console(sw(), sh(), color::BLACK, color::WHITE);
        root.fg = color::WHITE;
        draw_frame(&mut root, 0, 0, l.left_w, sh());
        draw_frame(&mut root, sw() - l.right_w, 0, l.right_w, sh());
        self.draw_battle_map(&mut root, &l);
        self.draw_battle_left(&mut root, &l);
        self.draw_battle_right(&mut root, &l);
        if let Some(fp) = &self.battle_anim.fp_window {
            let x = (l.map_x + l.map_w / 2 + 2).min(sw() - fp.w);
            Console::blit(fp, 0, 0, fp.w, fp.h, &mut root, x, (sh() / 2 - fp.h / 2).max(0));
        }
        self.draw_attack_window(&mut root);
        if let Some((m, x, y)) = &self.msg {
            Console::blit(m, 0, 0, m.w, m.h, &mut root, *x, *y);
        }
        self.root = root;
    }

    fn draw_battle_map(&self, root: &mut Console, l: &BattleLayout) {
        let s = self.sc();
        let snow = ["Snow", "Deep Snow"].contains(&self.cd().weather.ground.as_str());
        let bg_col = if snow { Color(158, 158, 158) } else { Color(0, 64, 0) };
        let rows = l.hex_rows();
        let outline = |root: &mut Console, h: (i32, i32), fg: Color, keep_bg: bool| {
            let (cx, cy) = l.plot(h.0, h.1);
            let n = rows.len();
            for (r, &(dx, dy, w)) in rows.iter().enumerate() {
                let edge_row = r == 0 || r == n - 1;
                for x in 0..w {
                    let (px, py) = (cx + dx + x, cy + dy);
                    let edge = edge_row || x == 0 || x == w - 1;
                    if edge {
                        let bg = if keep_bg { root.char_bg(px, py) } else { bg_col };
                        root.put_char_ex(px, py, 250, fg, bg);
                    } else if !keep_bg {
                        root.put_char_ex(px, py, 0, color::WHITE, bg_col);
                    }
                }
            }
        };
        let map_hexes: Vec<(i32, i32)> = s.hexes.iter().copied().filter(|h| distance(0, 0, h.0, h.1) <= 3).collect();
        for &h in &map_hexes {
            outline(root, h, Color(0, 102, 0), false);
            let (cx, cy) = l.plot(h.0, h.1);
            root.put_char_ex(cx, cy, 250, color::WHITE, bg_col);
        }

        // field of view (command phase) or the weapon's arc (shooting)
        let pl = s.player;
        let blue = Color(0, 0, 255);
        if s.phase == PHASE_COMMAND {
            if let Some(p) = s.units[pl].positions.get(s.selected_position) {
                for &h in &p.visible_hexes {
                    outline(root, h, blue, true);
                }
            }
        } else if s.phase == PHASE_SHOOTING {
            if let Some(w) = s.selected_weapon.and_then(|w| s.units[pl].weapons.get(w)) {
                for &h in &w.covered_hexes {
                    if distance(0, 0, h.0, h.1) <= 3 {
                        outline(root, h, blue, true);
                    }
                }
            }
        }

        // units: the top of each stack
        for &h in &map_hexes {
            let st = s.stack(h);
            let Some(&top) = st.first() else { continue };
            self.draw_unit(root, l, top, bg_col);
            if st.len() > 1 {
                let u = &s.units[top];
                let facing = u.turret_facing.unwrap_or(3);
                let mut ym = if [5, 0, 1].contains(&facing) { 1 } else { -1 };
                if u.battle.overrun {
                    ym -= 1;
                }
                let (x, y) = l.plot(u.hx, u.hy);
                let n = st.len().to_string();
                root.fg = color::GREY;
                root.bg = bg_col;
                root.print_ex_bg(x, y + ym, Align::Center, &n);
                root.bg = color::BLACK;
            }
        }
        for (u, cells) in &self.battle_anim.moves {
            if let Some(&(x, y)) = cells.first() {
                let col = if s.units[*u].owning_player == 1 { ENEMY_UNIT_COL } else { ALLIED_UNIT_COL };
                root.put_char_ex(x, y, self.display_char(*u), col, bg_col);
            }
        }

        // the target reticle
        if s.phase == PHASE_SHOOTING {
            if let Some(t) = s.selected_weapon.and_then(|w| s.units[pl].weapons.get(w)).and_then(|w| w.selected_target) {
                let (x, y) = l.plot(s.units[t].hx, s.units[t].hy);
                let (rx, ry) = (l.k / 2, (l.k / 4).max(1));
                for (dx, dy, ch) in [(-rx, -ry, 218), (rx, -ry, 191), (-rx, ry, 192), (rx, ry, 217)] {
                    let bg = root.char_bg(x + dx, y + dy);
                    root.put_char_ex(x + dx, y + dy, ch, color::RED, bg);
                }
            }
        }

        // animations
        let a = &self.battle_anim;
        let put = |root: &mut Console, x: i32, y: i32, ch: u16, fg: Color| {
            if x >= l.map_x && x < l.map_x + l.map_w && y >= l.map_y && y < l.map_y + l.map_h {
                let bg = root.char_bg(x, y);
                root.put_char_ex(x, y, ch, fg, bg);
            }
        };
        for &(x, y, life) in &a.rain {
            put(root, x, y, if life == 0 { 111 } else { 124 }, color::LIGHT_BLUE);
        }
        for &(x, y, _) in &a.snow {
            put(root, x, y, 249, color::WHITE);
        }
        if let Some((dir, line)) = &a.air {
            if let Some(&(x, y)) = line.first() {
                put(root, x - 1, y, 196, color::LIGHT_GREY);
                put(root, x, y, 197, color::LIGHT_GREY);
                put(root, x + 1, y, 196, color::LIGHT_GREY);
                if *dir == -1 {
                    put(root, x, y + 1, 193, color::LIGHT_GREY);
                } else {
                    put(root, x, y - 1, 194, color::LIGHT_GREY);
                }
            }
        }
        if let Some(&(x, y)) = a.gun_line.first() {
            put(root, x, y, 250, color::WHITE);
        }
        if let Some((line, _)) = &a.small_arms {
            if line.len() > 1 {
                let n = (a.last.map_or(0, |t| t.elapsed().subsec_nanos() as usize) % (line.len() - 1)) + 1;
                put(root, line[n].0, line[n].1, 250, color::YELLOW);
            }
        }
        if let Some(((x, y), life)) = a.bomb {
            let col = if 3 & life == 0 {
                color::RED
            } else if 2 & life == 0 {
                color::YELLOW
            } else {
                color::BLACK
            };
            put(root, x, y, 42, col);
        }
        if let Some(((x, y), life)) = a.grenade {
            let col = [color::RED, color::YELLOW, color::BLACK][(life % 3) as usize];
            put(root, x + life % 3 - 1, y + (life / 3) % 3 - 1, 250, col);
        }
        if let Some(((x, y), life)) = a.flame {
            let fg = [color::LIGHT_RED, color::LIGHT_YELLOW, color::GREY][(life % 3) as usize];
            let bg = [color::RED, color::YELLOW, color::BLACK][((life + 1) % 3) as usize];
            let (px, py) = (x + life % 3 - 1, y + (life / 3) % 3 - 1);
            root.put_char_ex(px, py, 177, fg, bg);
        }
        if let Some(h) = a.highlight {
            outline(root, h, color::LIGHT_BLUE, true);
            if a.flash {
                let (cx, cy) = l.plot(h.0, h.1);
                let _ = (cx, cy);
            }
        }
    }

    /// `GetDisplayChar`
    fn display_char(&self, i: usize) -> u16 {
        let s = self.sc();
        let u = &s.units[i];
        if i == s.player {
            return b'@' as u16;
        }
        if u.owning_player == 1 && !u.spotted {
            return b'?' as u16;
        }
        match u.category() {
            "Infantry" => 176,
            "Train Car" => 7,
            "Gun" => match u.facing {
                None => b'!' as u16,
                Some(_) if !u.battle.deployed => 124,
                Some(f) if [5, 0, 1].contains(&f) => 232,
                Some(_) => 233,
            },
            "Vehicle" => {
                if u.turret_facing.is_none() {
                    249
                } else {
                    9
                }
            }
            _ => b'!' as u16,
        }
    }

    /// `DrawMe`: a unit with its terrain, smoke, gun and cover.
    fn draw_unit(&self, root: &mut Console, l: &BattleLayout, i: usize, bg_col: Color) {
        let s = self.sc();
        let u = &s.units[i];
        if !u.alive {
            return;
        }
        let (x, mut y) = l.plot(u.hx, u.hy);
        if u.battle.overrun {
            y -= 1;
        }
        let hidden = u.owning_player == 1 && !u.spotted;
        if let (Some(t), false) = (&u.terrain, hidden) {
            let mut g = crate::rng::Rng::new(u.battle.terrain_seed as u64 * 7919 + 1);
            for loc in engine("GREEBLE_LOCATIONS").as_array().into_iter().flatten() {
                let (xm, ym) = (int(&loc[0]) * 2, int(&loc[1]));
                let (ch, col) = match t.as_str() {
                    "Open Ground" => {
                        if g.int(1, 9) <= 2 {
                            continue;
                        }
                        let c = g.int(0, 20) as u8;
                        (46, Color(30 + c, 90 + c, 20 + c))
                    }
                    "Broken Ground" => {
                        if g.int(1, 9) <= 2 {
                            continue;
                        }
                        let c = g.int(10, 40) as u8;
                        (if g.int(1, 10) <= 4 { 247 } else { 240 }, Color(70 + c, 60 + c, 40 + c))
                    }
                    "Brush" => {
                        if g.int(1, 9) <= 3 {
                            continue;
                        }
                        let col = Color(0, g.int(80, 120) as u8, 0);
                        (if g.int(1, 10) <= 6 { 15 } else { 37 }, col)
                    }
                    "Woods" => {
                        if g.int(1, 9) == 1 {
                            continue;
                        }
                        (6, Color(0, g.int(100, 170) as u8, 0))
                    }
                    "Wooden Buildings" => {
                        let c = g.int(10, 40) as u8;
                        (249, Color(70 + c, 60 + c, 40 + c))
                    }
                    "Hills" => {
                        if g.int(1, 9) <= 3 {
                            continue;
                        }
                        let c = g.int(10, 40) as u8;
                        (220, Color(20 + c, 110 + c, 20 + c))
                    }
                    "Fields" => {
                        if g.int(1, 9) == 1 {
                            continue;
                        }
                        let c = g.int(120, 190) as u8;
                        (176, Color(c, c, 0))
                    }
                    "Marsh" => {
                        if g.int(1, 9) == 1 {
                            continue;
                        }
                        (176, Color(45, 0, 180))
                    }
                    "Rubble" => {
                        if g.int(1, 9) <= 2 {
                            continue;
                        }
                        let ch = if g.int(1, 3) == 1 { 249 } else { 250 };
                        let c = g.int(120, 190) as u8;
                        (ch, Color(c, c, c))
                    }
                    _ => continue,
                };
                root.put_char_ex(x + xm, y + ym, ch, col, bg_col);
            }
        }
        let col = if u.owning_player == 1 {
            if s.units[s.player].battle.los.get(&i) == Some(&false) { color::GREY } else { ENEMY_UNIT_COL }
        } else if i == s.player {
            color::WHITE
        } else {
            ALLIED_UNIT_COL
        };
        if u.stat("class") == Some("Armoured Train Car") && !hidden {
            for x1 in x - 4..=x + 4 {
                root.put_char_ex(x1, y, 35, color::DARK_GREY, bg_col);
            }
            for x1 in x - 1..=x + 1 {
                root.put_char_ex(x1, y, 219, color::GREY, bg_col);
            }
        }
        root.put_char_ex(x, y, self.display_char(i), col, bg_col);
        if u.smoke > 0 {
            let sc = if u.smoke == 1 { color::DARK_GREY } else { color::DARKER_GREY };
            for (dx, dy) in [(-1, -1), (1, -1), (-1, 1), (1, 1)] {
                root.put_char_ex(x + dx, y + dy, 247, color::LIGHT_GREY, sc);
            }
        }
        if hidden {
            return;
        }
        let draw_turret = !(u.category() == "Infantry" || (u.category() == "Gun" && !u.battle.deployed) || u.weapons.is_empty());
        if draw_turret {
            if let Some(f) = u.turret_facing.or(u.facing) {
                let pd = &engine("PLOT_DIR")[f as usize];
                let ch = int(&engine("TURRET_CHAR")[f as usize]) as u16;
                root.put_char_ex(x + int(&pd[0]), y + int(&pd[1]), ch, col, bg_col);
            }
        }
        let cover = if u.battle.dug_in {
            Some((174, 175, color::DARK_SEPIA))
        } else if u.battle.entrenched {
            Some((91, 93, color::DARK_SEPIA))
        } else if u.battle.fortified {
            Some((249, 249, color::GREY))
        } else {
            None
        };
        if let Some((l1, r1, c)) = cover {
            root.put_char_ex(x - 1, y, l1, c, bg_col);
            root.put_char_ex(x + 1, y, r1, c, bg_col);
        }
    }

    /// The left column: the player's tank, the crew (`UpdateCrewInfoCon`),
    /// the commands (`UpdateCmdCon`).
    fn draw_battle_left(&self, root: &mut Console, l: &BattleLayout) {
        let s = self.sc();
        let pl = s.player;
        let u = &s.units[pl];
        let w = l.left_w - 2;
        let mut y = 1;
        if sh() >= 45 {
            display_my_info(u, root, 1, 1, true);
            y = 17;
        } else {
            root.fg = color::LIGHTER_BLUE;
            root.print_name(1, y, Align::Left, &u.unit_id);
            let mut status = Vec::new();
            if u.immobilized {
                status.push("Immob.".to_string());
            } else if u.bogged {
                status.push("Bogged".into());
            }
            if let Some(&d) = u.hull_down.first() {
                status.push(format!("HD{}", crate::glyph::to_char(crate::hexes::arrow(d))));
            }
            if u.smoke > 0 {
                status.push(format!("Smk{}", u.smoke));
            }
            if u.moving {
                status.push("Mov".into());
            }
            root.fg = color::LIGHT_GREY;
            root.print_ex(w, y, Align::Right, &status.join(" "));
            y += 1;
            root.fg = color::DARK_GREEN;
            root.print_ex(1, y, Align::Left, u.terrain.as_deref().unwrap_or(""));
            y += 1;
        }
        // the crew, three rows a position
        for (n, p) in u.positions.iter().enumerate() {
            if n == s.selected_position && (s.phase == PHASE_COMMAND || s.phase == PHASE_CREW_ACTION) {
                root.bg = color::DARKER_BLUE;
                root.rect(1, y, w, 3, true);
                root.bg = color::BLACK;
            }
            root.fg = color::LIGHT_BLUE;
            if p.crewman.as_ref().is_some_and(|c| c.normal_position != p.name) {
                root.fg = color::LIGHT_RED;
            }
            root.print_ex(1, y, Align::Left, &p.name);
            root.fg = color::WHITE;
            let loc = p.location.clone().unwrap_or_default();
            let loc = if p.name.len() + loc.len() + 1 > w as usize { loc.chars().take(1).collect() } else { loc };
            root.print_ex(w, y, Align::Right, &loc);
            match &p.crewman {
                None => root.print_ex(1, y + 1, Align::Left, "Empty"),
                Some(c) => {
                    let name = if c.nickname.is_empty() { c.name(NameForm::Initial) } else { format!("\"{}\"", c.nickname) };
                    root.print_name(1, y + 1, Align::Left, &name);
                    root.print_ex(w, y + 1, Align::Right, if !p.hatch { "--" } else if c.ce { "CE" } else { "BU" });
                    if !c.alive {
                        root.fg = color::DARK_GREY;
                        root.print_ex(1, y + 2, Align::Left, "Dead");
                    } else {
                        if c.injury.iter().any(|(_, v)| v.as_deref() == Some("Critical")) {
                            root.fg = color::LIGHT_RED;
                            root.print_ex(1, y + 2, Align::Left, "Critical Injury");
                        } else {
                            root.fg = color::DARK_YELLOW;
                            root.print_ex(1, y + 2, Align::Left, &c.current_cmd);
                        }
                        if c.condition != "Good Order" {
                            root.fg = color::LIGHT_GREY;
                            root.print_ex(w, y + 2, Align::Right, &c.condition);
                        }
                    }
                }
            }
            root.fg = color::WHITE;
            y += 3;
        }
        root.fg = color::DARKER_GREY;
        root.hline(1, y, w);
        y += 1;
        self.draw_battle_commands(root, 1, y, w);
    }

    /// `UpdateCmdCon`
    fn draw_battle_commands(&self, root: &mut Console, x: i32, y: i32, _w: i32) {
        let s = self.sc();
        if s.active_player == 1 || s.advance_phase {
            return;
        }
        let k = |c: char| self.enkey(c).to_ascii_uppercase().to_string();
        let ks = |a: char, b: char| self.enkeys(a, b);
        let lines: Vec<(String, &str)> = match s.phase {
            PHASE_COMMAND => vec![(ks('w', 's'), "Select Position"), (ks('a', 'd'), "Select Command"), ("H".into(), "Open/Shut Hatch"), (k('f'), "Crewman Menu")],
            PHASE_CREW_ACTION => {
                let mut v = vec![(ks('w', 's'), "Select Position")];
                let pos = &s.units[s.player].positions[s.selected_position];
                if pos.crewman.as_ref().is_some_and(|c| c.current_cmd == "Manage Ready Rack") {
                    v.push((ks('q', 'e'), "Select Weapon"));
                    let gun = s.selected_weapon.and_then(|w| s.units[s.player].weapons.get(w)).is_some_and(|w| w.stat("type") == Some("Gun"));
                    if gun {
                        v.push((ks('d', 'a'), "Add/Remove Shell"));
                        v.push((k('c'), "Cycle Ammo Type"));
                    }
                }
                v
            }
            PHASE_MOVEMENT => vec![(ks('w', 's'), "Forward/Reverse"), (ks('a', 'd'), "Pivot Hull"), ("H".into(), "Attempt HD"), (k('r'), "Reposition")],
            PHASE_SHOOTING => vec![
                (ks('w', 's'), "Select Weapon"),
                (ks('a', 'd'), "Select Target"),
                (ks('q', 'e'), "Rotate Turret"),
                (k('c'), "Cycle Ammo Type"),
                (k('r'), "Toggle RR Use"),
                (k('f'), "Fire at Target"),
            ],
            _ => vec![],
        };
        let mut yy = y;
        for (key, label) in &lines {
            key_line(root, x, x + 6, yy, key, label, color::LIGHT_GREY);
            yy += 1;
        }
        key_line(root, x, x + 6, yy, "Space", "End Phase", color::LIGHT_GREY);
    }

    /// The right column: time and phase, context, weather, unit info.
    fn draw_battle_right(&self, root: &mut Console, l: &BattleLayout) {
        let s = self.sc();
        let x0 = sw() - l.right_w + 1;
        let w = l.right_w - 2;
        let xm = x0 + w / 2;
        let cd = self.cd();
        root.fg = color::WHITE;
        root.print_ex(xm, 1, Align::Center, &date_text(&self.camp().today));
        root.bg = color::DARKER_YELLOW;
        root.rect(x0, 2, w, 1, true);
        let start = text(&self.camp().week()["day_start"]);
        let mut it = start.split(':').map(|x| x.parse::<i32>().unwrap_or(0));
        let (sh_, sm) = (it.next().unwrap_or(0), it.next().unwrap_or(0));
        let minutes = (cd.day_clock.0 - sh_) * 60 + cd.day_clock.1 - sm;
        root.bg = color::DARK_YELLOW;
        root.rect(x0, 2, (w as f64 * minutes as f64 / cd.day_length.max(1) as f64) as i32, 1, true);
        root.bg = color::BLACK;
        root.print_ex(xm, 2, Align::Center, &format!("{:02}:{:02}", cd.day_clock.0, cd.day_clock.1));
        let phase_col = &engine("SCEN_PHASE_COL")[s.phase as usize];
        root.fg = Color(int(&phase_col[0]) as u8, int(&phase_col[1]) as u8, int(&phase_col[2]) as u8);
        let phase = text(&engine("SCEN_PHASE_NAMES")[s.phase as usize]);
        let phase = format!("{phase} Phase");
        let phase = if phase.len() as i32 > w { phase.trim_end_matches(" Phase").to_string() } else { phase };
        root.print_ex(xm, 3, Align::Center, &phase);
        let pu = &s.units[s.player];
        root.fg = if pu.spotted { color::LIGHT_GREY } else { color::GREY };
        root.print_ex(xm, 4, Align::Center, if pu.spotted { "Spotted" } else { "Unspotted" });
        root.fg = color::WHITE;
        root.hline(x0, 5, w);

        // context
        let mut ctx = new_console(w, 10, color::BLACK, color::WHITE);
        self.draw_context(&mut ctx);
        Console::blit(&ctx, 0, 0, w, 10, root, x0, 6);
        root.fg = color::WHITE;
        root.hline(x0, 16, w);

        // weather
        let wx = &cd.weather;
        root.bg = color::DARK_BLUE;
        root.rect(x0, 17, w, 1, true);
        root.fg = color::WHITE;
        root.print_ex(xm, 17, Align::Center, &wx.cloud_cover);
        let (fg, bg) = match wx.precipitation.as_str() {
            "Rain" | "Heavy Rain" => (color::LIGHT_BLUE, color::DARK_BLUE),
            "None" => (color::WHITE, color::DARK_BLUE),
            _ => (color::LIGHT_GREY, color::DARK_GREY),
        };
        root.bg = bg;
        root.fg = fg;
        root.rect(x0, 18, w, 1, true);
        if wx.precipitation != "None" {
            root.print_ex(xm, 18, Align::Center, &wx.precipitation);
        }
        let (fg, bg) = match wx.ground.as_str() {
            "Muddy" => (color::GREY, color::DARKER_SEPIA),
            "Snow" | "Deep Snow" => (color::LIGHT_BLUE, color::GREY),
            _ => (color::WHITE, color::DARK_SEPIA),
        };
        root.fg = fg;
        root.bg = bg;
        root.rect(x0, 19, w, 1, true);
        root.print_ex(xm, 19, Align::Center, &wx.ground);
        root.bg = color::BLACK;
        root.fg = color::WHITE;
        root.hline(x0, 20, w);

        self.draw_unit_info(root, x0, 21, w, l);
    }

    /// `UpdateContextCon`
    fn draw_context(&self, c: &mut Console) {
        let s = self.sc();
        if s.advance_phase {
            return;
        }
        let w = c.w;
        let pl = s.player;
        let pu = &s.units[pl];
        match s.phase {
            PHASE_COMMAND => {
                let Some(cr) = pu.positions.get(s.selected_position).and_then(|p| p.crewman.as_ref()) else { return };
                let col = &engine("SCEN_PHASE_COL")[s.phase as usize];
                c.fg = Color(int(&col[0]) as u8, int(&col[1]) as u8, int(&col[2]) as u8);
                c.print_ex(0, 0, Align::Left, &cr.current_cmd);
                c.fg = color::LIGHT_GREY;
                let desc = defs::defs().crew_commands.get(&cr.current_cmd).map(|d| text(&d["desc"])).unwrap_or("");
                for (n, line) in wrap(desc, w as usize, "").iter().take(8).enumerate() {
                    c.print_ex(0, 2 + n as i32, Align::Left, line);
                }
            }
            PHASE_MOVEMENT => {
                let b = &pu.battle;
                c.fg = color::WHITE;
                c.print_ex(5, 0, Align::Left, "Success");
                c.print_ex(14, 0, Align::Left, "Bog");
                c.print_ex(0, 2, Align::Left, "Fwd");
                c.print_ex(0, 4, Align::Left, "Rev");
                c.print_ex(0, 6, Align::Left, "Pivot");
                c.fg = color::LIGHT_GREY;
                c.print_ex(10, 2, Align::Right, &format!("{}%", py_f(b.forward_move_chance)));
                c.print_ex(16, 2, Align::Right, &format!("{}%", py_f(b.bog_chance)));
                c.print_ex(10, 4, Align::Right, &format!("{}%", py_f(b.reverse_move_chance)));
                c.print_ex(16, 4, Align::Right, &format!("{}%", py_f(round1(b.bog_chance * 1.5))));
                c.print_ex(10, 6, Align::Right, "100%");
                c.print_ex(16, 6, Align::Right, &format!("{}%", py_f(round1(b.bog_chance * 0.25))));
            }
            PHASE_CREW_ACTION => {
                let Some(cr) = pu.positions.get(s.selected_position).and_then(|p| p.crewman.as_ref()) else { return };
                if cr.current_cmd != "Manage Ready Rack" {
                    return;
                }
                let Some(w) = s.selected_weapon.and_then(|w| pu.weapons.get(w)) else { return };
                c.bg = color::DARKEST_RED;
                c.rect(0, 0, c.w, 1, true);
                c.fg = color::WHITE;
                c.print_ex(0, 0, Align::Left, w.name());
                c.bg = color::BLACK;
                if w.stat("type") == Some("Gun") {
                    w.display_ammo(c, 0, 1, false);
                }
            }
            PHASE_SHOOTING => {
                let Some(wi) = s.selected_weapon else { return };
                let Some(w) = pu.weapons.get(wi) else { return };
                c.bg = color::DARKEST_RED;
                c.rect(0, 0, c.w, 1, true);
                c.fg = color::WHITE;
                c.print_ex(0, 0, Align::Left, w.name());
                if let Some(m) = w.stat("mount") {
                    c.fg = color::LIGHT_GREY;
                    c.print_ex(c.w - 1, 0, Align::Right, m);
                }
                c.bg = color::BLACK;
                if w.broken {
                    c.fg = color::LIGHT_RED;
                    c.print_ex(0, 1, Align::Left, "BROKEN");
                    return;
                } else if w.jammed {
                    c.fg = color::LIGHT_RED;
                    c.print_ex(0, 1, Align::Left, "JAMMED");
                    return;
                }
                if let (Some(t), Some((ac, level))) = (w.selected_target, w.acquired_target) {
                    if ac == t {
                        c.fg = color::LIGHT_BLUE;
                        c.print_ex(0, 7, Align::Left, if level == 1 { "Acquired Target+" } else { "Acquired Target" });
                    }
                }
                if w.stat("type") == Some("Gun") {
                    w.display_ammo(c, 0, 1, false);
                }
                if w.fired {
                    c.fg = color::RED;
                    c.print_ex(0, 6, Align::Left, "Fired");
                    return;
                }
                c.fg = color::WHITE;
                c.print_ex(c.w - 1, 1, Align::Right, "RoF");
                c.fg = color::LIGHT_GREY;
                let ch = self.rof_chance(pl, wi);
                c.print_ex(c.w - 1, 2, Align::Right, &if ch > 0.0 { format!("{}%", py_f(ch)) } else { "N/A".into() });
                if let Some(t) = w.selected_target {
                    let r = self.check_attack(pl, wi, t, false);
                    if r.is_empty() {
                        c.fg = color::LIGHT_BLUE;
                        c.print_ex(0, 8, Align::Left, "Ready to fire!");
                    } else {
                        c.fg = color::RED;
                        for (n, line) in wrap(&r, c.w as usize, "").iter().take(2).enumerate() {
                            c.print_ex(0, 8 + n as i32, Align::Left, line);
                        }
                    }
                }
            }
            PHASE_ALLIED_ACTION | PHASE_ENEMY_ACTION => {
                c.fg = color::LIGHT_GREY;
                for (n, line) in wrap(&s.context_text, c.w as usize, "").iter().take(10).enumerate() {
                    c.print_name(0, n as i32, Align::Left, line);
                }
            }
            _ => {}
        }
    }

    /// The hex the unit info is about: under the mouse (as in the
    /// original), else the selected weapon's target.
    fn info_hex(&self, l: &BattleLayout) -> Option<(i32, i32)> {
        if self.battle_anim.mouse_seen {
            if let Some(h) = l.hex_at(self.scr.mouse.cx, self.scr.mouse.cy) {
                return Some(h);
            }
        }
        let s = self.sc();
        let pu = &s.units[s.player];
        s.selected_weapon.and_then(|w| pu.weapons.get(w)).and_then(|w| w.selected_target).map(|t| (s.units[t].hx, s.units[t].hy))
    }

    /// `UpdateUnitInfoCon`, in the right column.
    fn draw_unit_info(&self, root: &mut Console, x0: i32, y0: i32, w: i32, l: &BattleLayout) {
        let s = self.sc();
        let Some(h) = self.info_hex(l) else {
            root.fg = color::DARK_GREY;
            for (n, line) in wrap("Mouseover a hex for details", w as usize, "").iter().enumerate() {
                root.print_ex(x0, y0 + n as i32, Align::Left, line);
            }
            return;
        };
        let range = match distance(0, 0, h.0, h.1) {
            0 => "0-120m.",
            1 => "120-480m.",
            2 => "480-960m.",
            _ => "960-1440m.",
        };
        root.fg = color::LIGHT_GREY;
        root.print_ex(x0, y0, Align::Left, &clip(&format!("Range: {range}"), w));
        let Some(&u) = s.stack(h).first() else { return };
        let unit = &s.units[u];
        let mut y = y0 + 1;
        if !s.los(s.player, u) {
            root.bg = color::GREY;
            root.fg = color::BLACK;
            root.rect(x0, y, w, 1, true);
            root.print_ex(x0, y, Align::Left, "LoS Blocked");
            root.bg = color::BLACK;
        }
        y += 1;
        if unit.owning_player == 1 && !unit.spotted {
            root.fg = UNKNOWN_UNIT_COL;
            root.print_ex(x0, y, Align::Left, "Unspotted Enemy");
            return;
        }
        root.fg = if u == s.player {
            color::WHITE
        } else if unit.owning_player == 0 {
            ALLIED_UNIT_COL
        } else {
            ENEMY_UNIT_COL
        };
        root.print_name(x0, y, Align::Left, &clip(&unit.unit_id, w));
        root.fg = color::LIGHT_GREY;
        let adj = defs::defs().nations.get(&unit.nation).map(|n| text(&n["adjective"])).unwrap_or("");
        root.print_name(x0, y + 1, Align::Left, &clip(adj, w));
        root.print_ex(x0, y + 2, Align::Left, &clip(unit.stat("class").unwrap_or(""), w));
        let y = y + 1;
        let mut st = Vec::new();
        if unit.immobilized {
            st.push("Immobilized");
        } else if unit.pinned {
            st.push("Pinned");
        } else if unit.moving {
            st.push("Moving");
        }
        if unit.routed {
            st.push("Routed");
        }
        if unit.fired {
            st.push("Fired");
        }
        root.print_ex(x0, y + 2, Align::Left, &clip(&st.join(" "), w));
        if let Some(t) = &unit.terrain {
            root.fg = color::DARK_GREEN;
            root.print_ex(x0, y + 3, Align::Left, &clip(t, w));
        }
        let cover = if unit.battle.dug_in {
            Some(("Dug-in", color::SEPIA))
        } else if unit.battle.entrenched {
            Some(("Entrenched", color::LIGHT_SEPIA))
        } else if unit.battle.fortified {
            Some(("Fortified", color::LIGHT_GREY))
        } else {
            None
        };
        if let Some((t, c)) = cover {
            root.fg = c;
            root.print_ex(x0 + w - 1, y + 4, Align::Right, t);
        }
        let y = y + 1;
        let mut x = x0;
        if let (Some(f), false) = (unit.facing, unit.category() == "Infantry") {
            root.put_char_ex(x, y + 4, b'H' as u16, color::LIGHT_GREY, color::DARKEST_GREY);
            root.put_char_ex(x + 1, y + 4, crate::hexes::arrow(f), color::LIGHT_GREY, color::DARKEST_GREY);
            x += 3;
        }
        if let Some(&d) = unit.hull_down.first() {
            root.fg = color::SEPIA;
            root.print_ex(x, y + 4, Align::Left, "HD");
            root.put_char_ex(x + 2, y + 4, crate::hexes::arrow(d), color::SEPIA, color::DARKEST_GREY);
            x += 4;
        }
        if unit.smoke > 0 {
            root.fg = color::GREY;
            root.print_ex(x, y + 4, Align::Left, &format!("Smoke {}", unit.smoke));
        }
        root.fg = color::DARK_GREY;
        if y + 6 < sh() - 1 {
            root.print_ex(x0, y + 5, Align::Left, &clip("Right click: details", w));
            root.print_ex(x0, y + 6, Align::Left, &clip("Wheel: cycle stack", w));
        }
    }

    /// `ShowUnitInfoWindow`
    fn show_unit_info_window(&mut self, i: usize) {
        let u = self.su(i).clone();
        let (w, h) = (27, (sh() - 2).min(30));
        let mut c = new_console(w, h, color::BLACK, color::WHITE);
        draw_frame(&mut c, 0, 0, w, h);
        display_my_info(&u, &mut c, 1, 1, true);
        let desc: String = u.stats.get("description").map(defs::strings).unwrap_or_default().concat();
        c.fg = color::LIGHT_GREY;
        for (n, line) in wrap(&desc, 25, "").iter().take((h - 19).max(0) as usize).enumerate() {
            c.print_name(1, 16 + n as i32, Align::Left, line);
        }
        key_line(&mut c, 7, 12, h - 2, "Esc", "Return", color::LIGHTER_GREY);
        let saved = self.root.clone();
        self.root.darken(0.7);
        Console::blit(&c, 0, 0, w, h, &mut self.root, sxm() - w / 2, (sh() - h) / 2);
        loop {
            self.flush();
            if self.input() && self.key().vk == Vk::Escape {
                break;
            }
        }
        self.root = saved;
    }

    // ---------------------------------------------------------------------
    // the loop

    /// `DoScenarioLoop`, until the battle is over or the player saves and
    /// quits.
    pub fn do_scenario_loop(&mut self) {
        if !self.sc().init_complete {
            self.setup_battle();
        }
        self.battle_anim.active = true;
        self.init_battle_animations();
        self.draw_battle();
        if self.cd().air_support_request || self.cd().arty_support_request {
            self.resolve_support_requests();
        }
        if self.sc().ambush {
            self.bmsg("You have been ambushed by enemy forces!", None);
        }
        crate::save::save_game(self);
        let mut mouse = (-1, -1);
        loop {
            if self.exiting {
                break;
            }
            if self.sc().finished {
                break;
            }
            self.flush();
            if self.sc().advance_phase {
                self.scm().advance_phase = false;
                self.advance_to_next_phase();
                if self.scenario.is_some() {
                    crate::save::save_game(self);
                    self.draw_battle();
                }
                continue;
            }
            self.battle_tick();
            if self.take_redraw() {
                self.draw_battle();
            }
            if self.scr.is_headless() && self.script.is_empty() {
                self.battle_autopilot();
            }
            let pressed = self.input();
            let m = (self.scr.mouse.cx, self.scr.mouse.cy);
            if m != mouse {
                if mouse != (-1, -1) || m != (0, 0) {
                    self.battle_anim.mouse_seen = true;
                }
                mouse = m;
                self.draw_battle();
            }
            let l = BattleLayout::new();
            let ms = self.scr.mouse;
            if ms.wheel_up || ms.wheel_down || ms.rbutton_pressed {
                if let Some(h) = l.hex_at(ms.cx, ms.cy) {
                    let st = self.sc().stack(h).clone();
                    if let Some(&top) = st.first() {
                        if ms.rbutton_pressed {
                            let u = self.su(top);
                            if !(u.owning_player == 1 && !u.spotted) {
                                self.show_unit_info_window(top);
                            }
                        } else if st.len() > 1 {
                            let stack = self.scm().stack_mut(h);
                            if ms.wheel_up {
                                stack.rotate_left(1);
                            } else {
                                stack.rotate_right(1);
                            }
                        }
                        self.draw_battle();
                    }
                }
            }
            if !pressed {
                continue;
            }
            if self.key().vk == Vk::Escape {
                self.show_game_menu();
                self.draw_battle();
                continue;
            }
            if self.sc().active_player == 1 {
                continue;
            }
            self.battle_key();
            self.draw_battle();
        }
        self.battle_anim.active = false;
        if self.scenario.as_ref().is_some_and(|s| s.finished) {
            self.end_battle();
        }
    }

    /// The set-up of a new battle (the start of `DoScenarioLoop`).
    pub(crate) fn setup_battle(&mut self) {
        let zone = self.sc().zone;
        self.start_battle(zone);
    }

    /// A key in the battle (`DoScenarioLoop`'s keyboard commands).
    pub(crate) fn battle_key(&mut self) {
        let k = self.mapped_char();
        let raw = self.key_char();
        let pl = self.pl();
        if self.key().vk == Vk::Space {
            self.scm().advance_phase = true;
            return;
        }
        if raw == '\t' || self.key().vk == Vk::Tab {
            // not in the original: the player's tank in full, for screens
            // too short to show it
            self.show_unit_info_window(pl);
            return;
        }
        let phase = self.sc().phase;
        if (phase == PHASE_COMMAND || phase == PHASE_CREW_ACTION) && (k == 'w' || k == 's') {
            let n = self.su(pl).positions.len();
            let s = self.scm();
            s.selected_position = if k == 'w' { (s.selected_position + n - 1) % n } else { (s.selected_position + 1) % n };
            return;
        }
        let sel = self.sc().selected_position;
        match phase {
            PHASE_COMMAND => {
                if k == 'a' || k == 'd' {
                    if self.su(pl).positions[sel].crewman.is_none() {
                        return;
                    }
                    crate::sound::play("command_select");
                    self.select_command(pl, sel, k == 'a');
                    self.update_visible_hexes(pl, sel);
                } else if raw == 'h' {
                    if self.su(pl).positions[sel].crewman.is_none() {
                        return;
                    }
                    if self.sum(pl).toggle_hatch(sel) {
                        crate::sound::play("hatch");
                        self.update_visible_hexes(pl, sel);
                    }
                } else if k == 'f' {
                    if self.su(pl).positions[sel].crewman.is_none() {
                        return;
                    }
                    self.battle_anim.active = false;
                    self.show_crewman_menu(CrewRef::Position(sel));
                    self.battle_anim.active = true;
                }
            }
            PHASE_CREW_ACTION => {
                let Some(c) = self.su(pl).positions[sel].crewman.as_ref() else { return };
                if c.current_cmd != "Manage Ready Rack" {
                    return;
                }
                if k == 'q' || k == 'e' {
                    self.select_weapon(k == 'e');
                    return;
                }
                let Some(w) = self.sc().selected_weapon else { return };
                if self.su(pl).weapons[w].stat("type") != Some("Gun") {
                    return;
                }
                if k == 'c' {
                    self.sum(pl).weapons[w].cycle_ammo();
                } else if k == 'a' || k == 'd' {
                    self.sum(pl).weapons[w].manage_rr(if k == 'a' { -1 } else { 1 });
                }
            }
            PHASE_MOVEMENT => {
                if k == 'w' || k == 's' {
                    self.move_player(k == 'w', false);
                } else if k == 'a' || k == 'd' {
                    self.pivot_player(k == 'd');
                } else if raw == 'h' {
                    let u = self.su(pl);
                    if u.hull_down.first().is_some_and(|&d| Some(d) == u.facing) {
                        return;
                    }
                    self.sum(pl).moving = true;
                    self.clear_acquired_targets(pl, false);
                    let u = self.su(pl).clone();
                    crate::sound::play_movement(&u, &mut self.rng);
                    if self.breakdown_check(pl) {
                        self.bmsg("Your vehicle stalls, making you unable to move further.", None);
                        self.scm().advance_phase = true;
                        return;
                    }
                    if self.check_for_hd(pl, true) {
                        self.bmsg("You move into a Hull Down position", None);
                    } else {
                        self.bmsg("You were unable to move into a Hull Down position", None);
                    }
                    if self.extra_move_check(pl) {
                        self.bmsg("You have moved swiftly enough to take another move action.", None);
                        return;
                    }
                    self.scm().advance_phase = true;
                } else if k == 'r' {
                    self.move_player(false, true);
                }
            }
            PHASE_SHOOTING => {
                if k == 'w' || k == 's' {
                    self.select_weapon(k == 's');
                } else if k == 'a' || k == 'd' {
                    self.cycle_target(k == 'd');
                } else if k == 'q' || k == 'e' {
                    self.rotate_player_turret(k == 'e');
                } else if k == 'c' {
                    if let Some(w) = self.sc().selected_weapon {
                        self.sum(pl).weapons[w].cycle_ammo();
                    }
                } else if k == 'r' {
                    if let Some(w) = self.sc().selected_weapon {
                        let wp = &mut self.sum(pl).weapons[w];
                        wp.using_rr = !wp.using_rr;
                    }
                } else if k == 'f' {
                    if let Some(w) = self.sc().selected_weapon {
                        let t = self.su(pl).weapons[w].selected_target;
                        self.attack(pl, w, t);
                    }
                }
            }
            _ => {}
        }
    }

    /// Headless runs: a player who sets sensible commands, drives forward
    /// now and then and fires at whatever is in reach.
    pub(crate) fn battle_autopilot(&mut self) {
        let pl = self.pl();
        let phase = self.sc().phase;
        let mut keys: Vec<char> = Vec::new();
        match phase {
            PHASE_COMMAND => {
                // the gunner on his gun, the rest as they are
                for p in 0..self.su(pl).positions.len() {
                    let Some(c) = self.su(pl).positions[p].crewman.clone() else { continue };
                    let want = if c.cmd_list.iter().any(|x| x == "Operate Gun") {
                        "Operate Gun"
                    } else if c.cmd_list.iter().any(|x| x == "Operate MG") {
                        "Operate MG"
                    } else if c.cmd_list.iter().any(|x| x == "Drive") && self.rng.int(1, 3) == 1 {
                        "Drive"
                    } else if c.cmd_list.iter().any(|x| x == "Reload") {
                        "Reload"
                    } else {
                        continue;
                    };
                    if let Some(cr) = self.sum(pl).positions[p].crewman.as_mut() {
                        cr.current_cmd = want.to_string();
                    }
                }
                keys.push(' ');
            }
            PHASE_MOVEMENT => {
                keys.push(*self.rng.choice(&['w', 's', 'a', 'd', 'h', ' ', ' ']));
            }
            PHASE_SHOOTING => {
                let s = self.sc();
                let Some(w) = s.selected_weapon else {
                    self.script.push_back(press(' '));
                    return;
                };
                let can = s.target_list.iter().copied().find(|&t| self.check_attack(pl, w, t, false).is_empty());
                match can {
                    Some(t) if self.rng.int(1, 4) > 1 => {
                        self.sum(pl).weapons[w].selected_target = Some(t);
                        keys.push('f');
                    }
                    _ => keys.push(*self.rng.choice(&['w', 'a', 'e', ' ', ' '])),
                }
            }
            _ => keys.push(' '),
        }
        self.script.extend(keys.into_iter().map(press));
    }
}

/// Text cut to `w` columns.
fn clip(t: &str, w: i32) -> String {
    t.chars().take(w.max(0) as usize).collect()
}

/// Python's `str()` of a float.
fn py_f(v: f64) -> String {
    format!("{v:?}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cday::tests::new_game;

    fn text_of(c: &Console) -> String {
        let mut s = String::new();
        for y in 0..c.h {
            for x in 0..c.w {
                s.push(crate::glyph::to_char(c.char_at(x, y)));
            }
            s.push('\n');
        }
        s
    }

    #[test]
    fn layouts_fit() {
        for (w, h) in [(100, 30), (130, 40), (160, 60), (220, 70)] {
            let l = BattleLayout::for_screen(w, h);
            assert!(l.map_x >= l.left_w && l.map_x + l.map_w <= w - l.right_w, "{w}x{h} {l:?}");
            assert!(l.map_y >= 0 && l.map_y + l.map_h <= h);
            for r in 0..4 {
                for h2 in crate::hexes::ring(0, 0, r) {
                    let (x, y) = l.plot(h2.0, h2.1);
                    assert_eq!(l.hex_at(x, y), Some(h2));
                }
            }
        }
    }

    /// Whole battles with the screen drawn at every step (to text, without
    /// a terminal); `ARMCOM2_SHOTS=dir` keeps a few frames.
    #[test]
    fn battles_draw() {
        let dir = std::env::var("ARMCOM2_SHOTS").ok();
        for seed in 1..=12u64 {
            let name = &defs::defs().campaigns[seed as usize % 10].0;
            let mut g = new_game(name, seed);
            g.render_headless = true;
            g.ammo_reload_menu();
            let zone = g.cd().player_unit_location;
            g.cdm().hexm(zone).controlled_by = 1;
            g.scenario = Some(Scenario::new(zone));
            g.setup_battle();
            g.battle_anim.active = true;
            if seed == 1 {
                // the windows, once each
                let pl = g.pl();
                let enemy = g.sc().order.iter().copied().find(|&u| g.su(u).owning_player == 1);
                if let (Some(e), Some(d)) = (enemy, &dir) {
                    g.sum(e).spotted = true;
                    if let Some(p) = g.calc_attack(pl, 0, e, false, false) {
                        g.show_attack(&p, true);
                        std::fs::write(format!("{d}/window_attack.txt"), text_of(&g.root)).unwrap();
                        let mut p = g.do_attack_roll(p);
                        g.show_attack(&p, false);
                        std::fs::write(format!("{d}/window_attack_roll.txt"), text_of(&g.root)).unwrap();
                        p.location = Some("Hull".into());
                        let ap = g.calc_ap(p, false);
                        g.show_attack(&ap, true);
                        std::fs::write(format!("{d}/window_ap.txt"), text_of(&g.root)).unwrap();
                        g.close_attack();
                    }
                    g.show_fp_window(e, 6, 12.5, 10.6, 5.3, Some((44.1, "Pinned".into())));
                    g.battle_anim.fp_window = g.battle_anim.fp_window.take().or_else(|| None);
                    let cols = vec![["OK".to_string(), "Bailed".into(), String::new()]; g.su(pl).positions.len()];
                    g.draw_bail_out(&cols, "Tank Burns", true);
                    std::fs::write(format!("{d}/window_bail_out.txt"), text_of(&g.root)).unwrap();
                }
            }
            let mut frames = 0;
            while g.scenario.as_ref().is_some_and(|s| !s.finished) && frames < 400 {
                if g.sc().advance_phase {
                    g.scm().advance_phase = false;
                    g.advance_to_next_phase();
                } else {
                    g.battle_autopilot();
                    while let Some(k) = g.script.pop_front() {
                        g.scr.key = k;
                        if g.sc().active_player == 0 {
                            g.battle_key();
                        }
                        if g.scenario.as_ref().is_none_or(|s| s.finished) {
                            break;
                        }
                    }
                }
                if g.scenario.is_some() {
                    g.draw_battle();
                    if let (Some(d), true) = (&dir, frames % 40 == 0) {
                        std::fs::write(format!("{d}/battle_{seed}_{frames}.txt"), text_of(&g.root)).unwrap();
                    }
                }
                frames += 1;
            }
        }
    }
}
