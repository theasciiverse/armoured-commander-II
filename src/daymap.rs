//! The campaign day map (`DoCampaignDayLoop` and the `CampaignDay`
//! functions it calls): the 41 zones, travel, recon, waiting, resupply,
//! the crew and the ammunition, the weather and the clock, random events,
//! zones won and lost, a new map region, and the day's end.
//!
//! The screen uses the whole terminal: the command menu in a 27-column
//! left column (with the tank's info above it when there are 41 rows or
//! more; at 30 rows it is in the Group tab), time, weather, the day's
//! mission and the zone info in a 29-column right column, and the map
//! between them. The original's zones are 7x9 square cells; here they are
//! terminal hexes, `c` columns and `r` rows apart (8 and 3 at 100x30,
//! bigger on bigger screens), each cell belonging to the zone its place
//! rounds to, so they tile exactly at any size. Edges are dotted as the
//! original's are; roads, rivers, bridges, the front line, the player and
//! known enemy strengths are drawn as it draws them.
//!

use crate::calendar::{CrewRef, display_crew};
use crate::campaign::display_my_info;
use crate::cday::{CdMapHex, cd_hexes, shuffle};
use crate::color::{self, Color};
use crate::console::{Align, Console};
use crate::defs::{self, engine, int, num, text};
use crate::game::*;
use crate::hexes::{adjacent_cd, constrain_dir, direction_to_adjacent_cd, get_line};
use crate::layout::*;
use crate::model::percentile;
use crate::rng::Rng;
use crate::ui::{Vk, wrap};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

pub const LEFT_W: i32 = 27;
pub const RIGHT_W: i32 = 29;

pub const DIRT_ROAD_COL: Color = Color(80, 50, 20);
pub const STONE_ROAD_COL: Color = Color(110, 110, 110);
/// The original's (0, 0, 140) is lost on a dark terminal; brighter here.
pub const RIVER_COL: Color = Color(20, 60, 190);
pub const ALLIED_UNIT_COL: Color = Color(120, 120, 255);
pub const ENEMY_UNIT_COL: Color = Color(255, 20, 20);

const ANIM_EVERY: Duration = Duration::from_millis(200);

/// Where the map and its hexes go on the screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DayLayout {
    pub map_x: i32,
    pub map_w: i32,
    pub map_h: i32,
    /// Columns between zone centres in a row, and rows between rows.
    pub c: i32,
    pub r: i32,
    /// The centre of zone (0, 0), in map cells.
    pub ox: i32,
    pub oy: i32,
}

impl DayLayout {
    pub fn new() -> Self {
        DayLayout::for_screen(sw(), sh())
    }

    pub fn for_screen(w: i32, h: i32) -> Self {
        let map_x = LEFT_W;
        let map_w = w - LEFT_W - RIGHT_W;
        let map_h = h;
        // the biggest hexes that fit, about the original's proportions (6 by
        // 5 square cells, so 12 columns to 5 rows)
        let (mut c, mut r) = (8, 3);
        for rr in [6, 5, 4] {
            let cc = ((rr as f64 * 2.4).round() as i32 + 1) / 2 * 2;
            if 5 * cc + 4 <= map_w && 9 * rr + 2 + hh(rr) - 2 <= map_h {
                (c, r) = (cc, rr);
                break;
            }
        }
        let content_w = 5 * c + 1;
        let ox = 2 + (map_w - 2 - content_w - 2).max(0) / 2 + c / 2;
        let content_h = 8 * r + 2 * hh(r) + 1;
        let oy = hh(r) + (map_h - 1 - content_h).max(0) / 2;
        DayLayout { map_x, map_w, map_h, c, r, ox, oy }
    }

    /// A zone's centre in map cells (`PlotCDHex`).
    pub fn plot(&self, hx: i32, hy: i32) -> (i32, i32) {
        (self.ox + self.c * hx + self.c / 2 * hy, self.oy + self.r * hy)
    }

    /// The zone a map cell is in, if any place: the cell's place in zone
    /// coordinates, rounded to the nearest zone.
    pub fn zone_at(&self, x: i32, y: i32) -> (i32, i32) {
        let fy = (y - self.oy) as f64 / self.r as f64;
        let fx = (x - self.ox) as f64 / self.c as f64 - fy / 2.0;
        let fz = -fx - fy;
        let (mut rx, ry, mut rz) = (fx.round(), fy.round(), fz.round());
        let (dx, dy, dz) = ((rx - fx).abs(), (ry - fy).abs(), (rz - fz).abs());
        let mut ry = ry;
        if dx > dy && dx > dz {
            rx = -ry - rz;
        } else if dz > dy {
            rz = -rx - ry;
        } else {
            ry = -rx - rz;
        }
        let _ = rz;
        (rx as i32, ry as i32)
    }
}

/// Half a hex's height in rows, above and below its centre.
fn hh(r: i32) -> i32 {
    (2 * r + 2) / 3
}

/// The map drawn once (terrain, roads, rivers, bridges), and where the
/// zones and bridges are on it.
pub struct DayMap {
    pub layout: DayLayout,
    pub con: Console,
    pub owner: Vec<Option<(i32, i32)>>,
    /// Cells on a zone edge: (cell, zone, the zone or none beyond).
    pub edges: Vec<((i32, i32), (i32, i32), Option<(i32, i32)>)>,
    pub bridges: Vec<(i32, i32)>,
}

impl DayMap {
    pub fn owner_at(&self, x: i32, y: i32) -> Option<(i32, i32)> {
        if x < 0 || y < 0 || x >= self.layout.map_w || y >= self.layout.map_h {
            return None;
        }
        self.owner[(y * self.layout.map_w + x) as usize]
    }

    /// The edge cells between a zone and its neighbour in direction `d`.
    pub fn edge_cells(&self, zone: (i32, i32), d: i32) -> Vec<(i32, i32)> {
        let other = adjacent_cd(zone.0, zone.1, d);
        self.edges
            .iter()
            .filter(|(_, a, b)| (*a == zone && *b == Some(other)) || (*a == other && *b == Some(zone)))
            .map(|(c, _, _)| *c)
            .collect()
    }

    /// Every edge cell of a zone, inside or out.
    pub fn outline(&self, zone: (i32, i32)) -> Vec<(i32, i32)> {
        self.edges.iter().filter(|(_, a, b)| *a == zone || *b == Some(zone)).map(|(c, _, _)| *c).collect()
    }
}

/// `UpdateCDMapCon`: the terrain map.
pub fn build_day_map(cd: &crate::cday::CampaignDay) -> DayMap {
    build_day_map_in(cd, DayLayout::new())
}

pub fn build_day_map_in(cd: &crate::cday::CampaignDay, l: DayLayout) -> DayMap {
    let (w, h) = (l.map_w, l.map_h);
    let mut con = Console::new(w, h);
    con.bg = color::BLACK;
    con.clear();
    let mut owner = vec![None; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let z = l.zone_at(x, y);
            if cd.on_map(z) {
                owner[(y * w + x) as usize] = Some(z);
            }
        }
    }
    let at = |x: i32, y: i32| -> Option<(i32, i32)> {
        if x < 0 || y < 0 || x >= w || y >= h { None } else { owner[(y * w + x) as usize] }
    };
    // edges: a cell whose right or lower neighbour is another zone, or
    // whose any neighbour is off the map
    let mut edges = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let Some(z) = at(x, y) else { continue };
            let mut others: Vec<Option<(i32, i32)>> = Vec::new();
            for (dx, dy) in [(1, 0), (0, 1)] {
                let o = at(x + dx, y + dy);
                if o != Some(z) && o.is_some() && !others.contains(&o) {
                    others.push(o);
                }
            }
            if others.is_empty() && [(-1, 0), (0, -1), (1, 0), (0, 1)].iter().any(|(dx, dy)| at(x + dx, y + dy).is_none()) {
                others.push(None);
            }
            for o in others {
                edges.push(((x, y), z, o));
            }
        }
    }

    let snow = ["Snow", "Deep Snow"].contains(&cd.weather.ground.as_str());
    let bg_col = if snow { Color(158, 158, 158) } else { Color(0, 64, 0) };
    // the original's dotted edges (0, 102, 0 on 0, 64, 0) read as lines in
    // its 8-pixel font, not in a terminal: a darker ground and brighter
    // dots, so each zone has an outline
    let edge_fg = if snow { Color(40, 80, 150) } else { Color(110, 170, 110) };
    let edge_bg = if snow { Color(118, 118, 118) } else { Color(0, 36, 0) };
    for y in 0..h {
        for x in 0..w {
            if at(x, y).is_some() {
                con.put_char_ex(x, y, 0, color::WHITE, bg_col);
            }
        }
    }
    for ((x, y), _, _) in &edges {
        con.put_char_ex(*x, *y, 250, edge_fg, edge_bg);
    }

    // terrain, from each zone's own seed
    let is_edge: std::collections::BTreeSet<(i32, i32)> = edges.iter().map(|e| e.0).collect();
    for (&(hx, hy), z) in &cd.map_hexes {
        let (cx, cy) = l.plot(hx, hy);
        let mut cells: Vec<(i32, i32)> = Vec::new();
        for y in cy - hh(l.r)..=cy + hh(l.r) {
            for x in cx - l.c..=cx + l.c {
                if at(x, y) == Some((hx, hy)) && !is_edge.contains(&(x, y)) {
                    cells.push((x, y));
                }
            }
        }
        let mut g = Rng::new(z.console_seed as u64 * 7919 + 1);
        let scaled = |n: i32| (n * cells.len() as i32 + 11) / 22;
        match z.terrain_type.as_str() {
            "Forest" => {
                for &(x, y) in &cells {
                    if g.int(1, 10) <= 4 {
                        continue;
                    }
                    let (ch, col) = if cd.weather.freezing && g.int(1, 2) == 1 {
                        let ch = if g.int(1, 3) == 1 { 124 } else { 24 };
                        (ch, Color(g.int(60, 100) as u8, 20, 20))
                    } else {
                        (6, Color(0, g.int(100, 170) as u8, 0))
                    };
                    con.put_char_ex(x, y, ch, col, bg_col);
                }
            }
            "Hills" => {
                let col = if snow {
                    let v = g.int(170, 210) as u8;
                    Color(v, v, v)
                } else {
                    Color(70, g.int(110, 150) as u8, 0)
                };
                // three hills, high, middle and low in the zone
                for row in [cy - hh(l.r) / 2 - 1, cy, cy + hh(l.r) / 2 + 1] {
                    let in_row: Vec<i32> = cells.iter().filter(|c| c.1 == row).map(|c| c.0).collect();
                    let starts: Vec<i32> = in_row.iter().copied().filter(|x| in_row.contains(&(x + 1))).collect();
                    if starts.is_empty() {
                        continue;
                    }
                    let x = *g.choice(&starts);
                    con.put_char_ex(x, row, 236, col, bg_col);
                    con.put_char_ex(x + 1, row, 237, col, bg_col);
                }
            }
            "Fields" => {
                for &(x, y) in &cells {
                    if cd.weather.freezing {
                        con.put_char_ex(x, y, 124, Color(50, 40, 20), bg_col);
                    } else {
                        let v = g.int(120, 190) as u8;
                        con.put_char_ex(x, y, 176, Color(v, v, 0), bg_col);
                    }
                }
            }
            "Marsh" | "Villages" => {
                let (n, ch, col) = if z.terrain_type == "Marsh" {
                    (g.int(7, 13), 176, Color(45, 0, 180))
                } else {
                    (g.int(5, 9), 249, Color(77, 77, 77))
                };
                let mut spots = cells.clone();
                shuffle(&mut spots, &mut g);
                for &(x, y) in spots.iter().take(scaled(n).max(1) as usize) {
                    con.put_char_ex(x, y, ch, col, bg_col);
                }
            }
            _ => {}
        }
    }

    // roads: each link once, from centre to centre
    let paint_road = |con: &mut Console, line: &[(i32, i32)], col: Color, stop_off_map: bool| {
        for &(x, y) in line {
            if at(x, y).is_none() {
                if stop_off_map {
                    break;
                }
                continue;
            }
            con.set_char_bg(x, y, col);
            if ![0, 249, 250].contains(&con.char_at(x, y)) {
                let cell = con.cell(x, y).unwrap();
                con.put_char_ex(x, y, 0, cell.fg, col);
            }
        }
    };
    for (&(hx, hy), z) in &cd.map_hexes {
        if !z.has_road() {
            continue;
        }
        let (x1, y1) = l.plot(hx, hy);
        for d in 0..3 {
            let Some(stone) = z.road_links[d as usize] else { continue };
            let n = adjacent_cd(hx, hy, d);
            if !cd.on_map(n) {
                continue;
            }
            let (x2, y2) = l.plot(n.0, n.1);
            paint_road(&mut con, &get_line(x1, y1, x2, y2, false), if stone { STONE_ROAD_COL } else { DIRT_ROAD_COL }, false);
        }
        // a zone on the edge with one road: the road goes on off the map.
        // The original counted only the links it drew from this zone (the
        // first three directions), so it drew these for zones with two
        // roads too, in the colour of the zone drawn before (display)
        let links: Vec<bool> = z.road_links.iter().flatten().copied().collect();
        if links.len() != 1 {
            continue;
        }
        if let Some(d) = (0..6).find(|&d| !cd.on_map(adjacent_cd(hx, hy, d))) {
            let n = adjacent_cd(hx, hy, d);
            let (x2, y2) = l.plot(n.0, n.1);
            paint_road(&mut con, &get_line(x1, y1, x2, y2, false), if links[0] { STONE_ROAD_COL } else { DIRT_ROAD_COL }, true);
        }
    }

    let mut map = DayMap { layout: l, con, owner, edges, bridges: Vec::new() };

    // rivers on zone edges, and bridges over them
    for (&zone, z) in &cd.map_hexes {
        for &d in &z.rivers {
            for (x, y) in map.edge_cells(zone, d) {
                map.con.put_char_ex(x, y, 0, color::WHITE, RIVER_COL);
            }
        }
    }
    for (&zone, z) in &cd.map_hexes {
        for &d in &z.bridges {
            let mut cells = map.edge_cells(zone, d);
            cells.sort();
            let ch = match d {
                0 | 3 => b'/',
                2 | 5 => b'\\',
                _ => b'-',
            } as u16;
            // the middle of the edge, as the original leaves out its ends
            let n = cells.len();
            let middle: Vec<(i32, i32)> = if n <= 2 { cells } else { cells[n / 2 - (n > 3) as usize..=n / 2].to_vec() };
            for (x, y) in middle {
                let bg = map.con.char_bg(x, y);
                map.con.put_char_ex(x, y, ch, color::DARK_SEPIA, bg);
                map.bridges.push((x, y));
            }
        }
    }

    // row letters and column numbers
    for hy in 0..9 {
        let hx = -(hy / 2);
        let (x, y) = l.plot(hx, hy);
        map.con.put_char_ex(x - l.c / 2 - 2, y, b'A' as u16 + hy as u16, color::LIGHT_GREEN, color::BLACK);
    }
    for hx in -4..=4 {
        let last = (0..9).rev().find(|&hy| cd.on_map((hx, hy)));
        let Some(hy) = last else { continue };
        let (x, y) = l.plot(hx, hy + 1);
        let y = (y - l.r + hh(l.r)).min(l.map_h - 1);
        map.con.put_char_ex(x, y, b'1' as u16 + (hx + 4) as u16, color::LIGHT_GREEN, color::BLACK);
    }
    map
}

/// The day map's looping animations (`InitAnimations`, `UpdateAnimCon`):
/// rain, snow, and a zone's outline flashing.
#[derive(Default)]
pub struct DayAnim {
    pub rain: Vec<(i32, i32, i32)>,
    pub snow: Vec<(i32, i32, i32)>,
    pub highlight: Option<(i32, i32)>,
    pub flash: bool,
    pub last: Option<Instant>,
    /// The weather panel's random clouds and drops, kept until the
    /// weather changes (the original drew them once per change).
    pub weather_seed: u64,
    /// The player's marker moving between zones.
    pub offset: (i32, i32),
}

impl Game {
    /// `InitAnimations`
    pub fn init_day_animations(&mut self) {
        let (w, h) = (self.day_map.as_ref().map_or(40, |m| m.layout.map_w), sh());
        let precip = self.cd().weather.precipitation.clone();
        let mut a = DayAnim { weather_seed: self.rng.int(1, i32::MAX) as u64, ..DayAnim::default() };
        let scale = |n: i32| (n * w * h / (36 * 52)).max(n / 2);
        match precip.as_str() {
            "Rain" | "Heavy Rain" => {
                let n = scale(if precip == "Heavy Rain" { 16 } else { 8 });
                for _ in 0..n {
                    a.rain.push((self.rng.int(0, w - 1), self.rng.int(0, h - 1), self.rng.int(1, 5)));
                }
            }
            "Light Snow" | "Snow" | "Blizzard" => {
                let n = scale(match precip.as_str() {
                    "Light Snow" => 4,
                    "Snow" => 8,
                    _ => 16,
                });
                for _ in 0..n {
                    a.snow.push((self.rng.int(0, w - 1), self.rng.int(0, h - 1), self.rng.int(4, 10)));
                }
            }
            _ => {}
        }
        self.day_anim = a;
    }

    /// `UpdateAnimCon`: move the rain and snow on a step, flash the
    /// highlighted zone.
    fn step_day_animations(&mut self) {
        let (w, h) = (self.day_map.as_ref().map_or(40, |m| m.layout.map_w), sh());
        let mut a = std::mem::take(&mut self.day_anim);
        for d in &mut a.rain {
            if d.2 == 0 {
                *d = (self.rng.int(0, w - 1), self.rng.int(0, h - 1), self.rng.int(1, 5));
            } else {
                d.1 += 1;
                d.2 -= 1;
            }
        }
        for f in &mut a.snow {
            if f.2 == 0 {
                *f = (self.rng.int(0, w - 1), self.rng.int(0, h - 1), self.rng.int(4, 10));
            } else {
                f.0 += self.rng.int(-1, 1);
                f.1 += 1;
                f.2 -= 1;
            }
        }
        a.flash = !a.flash;
        a.last = Some(Instant::now());
        self.day_anim = a;
    }

    /// The day map screen is up: the day map and its animations draw
    /// themselves while messages wait.
    pub fn day_tick(&mut self) {
        if self.day_map.is_none() || self.battle_anim.active {
            return;
        }
        if self.day_anim.last.is_none_or(|t| t.elapsed() >= ANIM_EVERY) {
            self.step_day_animations();
            self.draw_day();
        }
    }

    /// Lay the day map out again (a resize, a new region, new snow).
    pub fn rebuild_day_map(&mut self) {
        self.day_map = Some(build_day_map(self.cd()));
    }

    /// `UpdateCDDisplay`: the whole day screen onto the root console.
    pub fn draw_day(&mut self) {
        if self.day_map.as_ref().is_none_or(|m| m.layout != DayLayout::new()) {
            self.rebuild_day_map();
        }
        let mut root = new_console(sw(), sh(), color::BLACK, color::WHITE);
        self.draw_day_frame(&mut root);
        self.draw_day_map_layers(&mut root);
        self.draw_day_left(&mut root);
        self.draw_day_right(&mut root);
        if let Some((m, x, y)) = &self.msg {
            Console::blit(m, 0, 0, m.w, m.h, &mut root, *x, *y);
        }
        self.root = root;
    }

    fn draw_day_frame(&self, root: &mut Console) {
        root.fg = color::WHITE;
        draw_frame(root, 0, 0, LEFT_W, sh());
        draw_frame(root, sw() - RIGHT_W, 0, RIGHT_W, sh());
    }

    /// The map and the layers over it: the front line and targets, the
    /// enemy strengths and the player, the chosen direction, the weather.
    fn draw_day_map_layers(&self, root: &mut Console) {
        let map = self.day_map.as_ref().expect("a day map");
        let l = map.layout;
        let cd = self.cd();
        let mut m = map.con.clone();
        let put = |m: &mut Console, x: i32, y: i32, ch: u16, fg: Color| {
            let bg = m.char_bg(x, y);
            m.put_char_ex(x, y, ch, fg, bg);
        };

        // the front line, between friendly zones and enemy ones
        for (&zone, z) in &cd.map_hexes {
            if z.controlled_by != 0 {
                continue;
            }
            for d in 0..6 {
                let n = adjacent_cd(zone.0, zone.1, d);
                if cd.on_map(n) && cd.hex(n).controlled_by != 0 {
                    for (x, y) in map.edge_cells(zone, d) {
                        put(&mut m, x, y, 249, color::RED);
                    }
                }
            }
        }
        for (&(hx, hy), z) in &cd.map_hexes {
            let (x, y) = l.plot(hx, hy);
            if z.target_of_opportunity.is_some() {
                put(&mut m, x, y, 4, color::YELLOW);
            }
            if z.controlled_by != 0 && z.known_to_player {
                m.fg = color::RED;
                let bg = m.char_bg(x, y - 1);
                for (i, ch) in z.enemy_strength.to_string().chars().enumerate() {
                    m.put_char_ex(x + i as i32, y - 1, ch as u16, color::RED, bg);
                }
            }
        }
        let (px, py) = l.plot(cd.player_unit_location.0, cd.player_unit_location.1);
        let (px, py) = (px + self.day_anim.offset.0, py + self.day_anim.offset.1);
        m.put_char_ex(px, py, b'@' as u16, color::WHITE, color::BLACK);

        // the chosen direction (travel tab)
        if cd.active_menu == 3 {
            if let Some(d) = cd.selected_direction {
                let (hx, hy) = cd.player_unit_location;
                let n = adjacent_cd(hx, hy, d);
                if cd.on_map(n) {
                    let (x2, y2) = l.plot(n.0, n.1);
                    let line = get_line(px, py, x2, y2, false);
                    for &(x, y) in &line[1..line.len() - 1] {
                        m.put_char_ex(x, y, 250, color::GREEN, color::BLACK);
                    }
                    let (x, y) = *line.last().unwrap();
                    let arrow = int(&engine("CD_DIR_ARROW")[d as usize]) as u16;
                    m.put_char_ex(x, y, arrow, color::GREEN, color::BLACK);
                }
            }
        }

        // rain, snow, the flashing zone
        for &(x, y, life) in &self.day_anim.rain {
            if y < l.map_h {
                m.put_char_ex(x, y, if life == 0 { 111 } else { 124 }, color::LIGHT_BLUE, color::BLACK);
            }
        }
        for &(x, y, _) in &self.day_anim.snow {
            if x >= 0 && x < l.map_w && y < l.map_h {
                m.put_char_ex(x, y, 249, color::WHITE, color::BLACK);
            }
        }
        if let Some(zone) = self.day_anim.highlight {
            let ch = if self.day_anim.flash { 249 } else { 250 };
            for (x, y) in map.outline(zone) {
                m.put_char_ex(x, y, ch, color::LIGHT_BLUE, color::BLACK);
            }
        }
        Console::blit(&m, 0, 0, l.map_w, l.map_h, root, l.map_x, 0);
    }

    /// The left column: the tank (on tall screens) and `UpdateCDCommandCon`.
    fn draw_day_left(&self, root: &mut Console) {
        let tall = sh() >= 41;
        let mut top = 1;
        if tall {
            display_my_info(self.camp().pu(), root, 1, 1, false);
            root.fg = color::WHITE;
            root.hline(1, 12, LEFT_W - 2);
            top = 13;
        }
        let mut c = new_console(LEFT_W - 2, sh() - 1 - top, color::BLACK, color::WHITE);
        self.draw_cd_command(&mut c, tall);
        Console::blit(&c, 0, 0, c.w, c.h, root, 1, top);
    }

    fn draw_cd_command(&self, c: &mut Console, tall: bool) {
        let cd = self.cd();
        let camp = self.camp();
        let k = |ch: char| self.enkey(ch).to_ascii_uppercase().to_string();
        c.fg = TITLE_COL;
        c.print_ex(6, 0, Align::Left, "Command Menu");
        let mut x = 0;
        let mut last = color::BLACK;
        for item in engine("CD_MENU_LIST").as_array().into_iter().flatten() {
            let (label, n, col) = (text(&item[0]), int(&item[1]), &item[2]);
            let col = Color(int(&col[0]) as u8, int(&col[1]) as u8, int(&col[2]) as u8);
            last = col;
            c.bg = col;
            c.rect(x, 1, 2, 1, true);
            c.fg = ACTION_KEY_COL;
            c.print_ex(x, 1, Align::Left, &n.to_string());
            c.fg = color::WHITE;
            x += 2;
            if cd.active_menu == n {
                c.rect(x, 1, label.len() as i32 + 2, 1, true);
                c.print_ex(x, 1, Align::Left, label);
                x += label.len() as i32 + 2;
            }
        }
        c.bg = last;
        c.rect(x, 1, c.w - x, 1, true);
        c.bg = color::BLACK;
        let h = c.h;

        match cd.active_menu {
            1 => {
                if let Some(g) = cd.selected_gun {
                    let w = &camp.pu().weapons[g];
                    c.fg = color::WHITE;
                    c.print_ex(6, 3, Align::Left, w.name());
                    w.display_ammo(c, 6, 5, false);
                    key_line(c, 2, 6, 13, &format!("{}/{}", k('d'), k('a')), "Add/Remove to RR", color::LIGHTER_GREY);
                    key_line(c, 4, 6, 14, &k('c'), "Cycle Ammo Type", color::LIGHTER_GREY);
                    key_line(c, 4, 6, 15, &k('q'), "Cycle Selected Gun", color::LIGHTER_GREY);
                }
                c.fg = color::WHITE;
                c.print_ex(12, 18, Align::Center, "Request resupply");
                c.print_ex(12, 19, Align::Center, "and reserve units,");
                c.print_ex(12, 20, Align::Center, "30 mins.");
                key_line(c, 4, 6, 22, &k('r'), "Request Resupply", color::LIGHTER_GREY);
            }
            2 => {
                display_crew(camp.pu(), c, 0, 2, 25, Some(cd.selected_position));
                let y = h - 2;
                key_line(c, 0, 4, y, &self.enkeys('w', 's'), "Select", color::LIGHTER_GREY);
                key_line(c, 12, 14, y, "P", "Swap Position", color::LIGHTER_GREY);
                key_line(c, 0, 4, y + 1, &k('f'), "Crewman", color::LIGHTER_GREY);
                key_line(c, 12, 14, y + 1, "H", "Toggle Hatch", color::LIGHTER_GREY);
            }
            3 => self.draw_travel_menu(c),
            _ => {
                let mut y = 3;
                if !tall {
                    display_my_info(camp.pu(), c, 0, 3, false);
                    y = 15;
                }
                c.fg = TITLE_COL;
                c.print_ex(1, y, Align::Left, "Squad");
                c.fg = color::WHITE;
                c.print_name(2, y + 2, Align::Left, &camp.pu().unit_id);
                c.fg = color::LIGHTER_GREY;
                for (i, u) in cd.player_squad.iter().enumerate() {
                    c.print_name(2, y + 3 + i as i32, Align::Left, &u.unit_id);
                }
            }
        }
    }

    fn draw_travel_menu(&self, c: &mut Console) {
        let cd = self.cd();
        let week = self.camp().week();
        let k = |ch: char| self.enkey(ch).to_ascii_uppercase().to_string();
        c.fg = color::WHITE;
        c.print_ex(0, 3, Align::Left, "Air Support:");
        let air = if !week.contains_key("air_support_level") {
            "None".to_string()
        } else if cd.weather.cloud_cover == "Overcast" {
            "*Overcast*".to_string()
        } else {
            format!("{}%", py_float(cd.air_support_level))
        };
        c.print_ex(24, 3, Align::Right, &air);
        c.print_ex(0, 4, Align::Left, "Artillery Support:");
        let arty = if !week.contains_key("arty_support_level") {
            "None".to_string()
        } else {
            format!("{}%", py_float(cd.arty_support_level))
        };
        c.print_ex(24, 4, Align::Right, &arty);

        // the six directions around the player
        let (x1, y1) = (12, 8);
        c.fg = color::WHITE;
        c.put_char(x1, y1, b'@' as u16);
        for (d, cmd) in engine("CD_TRAVEL_CMDS").as_array().into_iter().flatten().enumerate() {
            let (key, x, y, ch) = (text(&cmd[0]), int(&cmd[1]), int(&cmd[2]), int(&cmd[3]));
            // the original's rose is five rows high; here three
            let y = y / 2;
            c.fg = if cd.selected_direction == Some(d as i32) { color::LIGHT_GREEN } else { ACTION_KEY_COL };
            let kc = self.enkey(key.chars().next().unwrap_or(' ')).to_ascii_uppercase();
            c.put_char(x1 + x, y1 + y, kc as u16);
            let ax = if d <= 2 { x + 1 } else { x - 1 };
            c.fg = color::DARK_GREEN;
            c.put_char(x1 + ax, y1 + y, ch as u16);
        }
        if cd.selected_direction.is_none() {
            c.fg = color::LIGHT_GREY;
            c.print_ex(13, y1 + 3, Align::Center, "Select a Direction");
        }
        let y0 = c.h - 4;
        key_line(c, 3, 10, y0, &k('i'), "Abandon Tank", color::LIGHTER_GREY);
        key_line(c, 3, 10, y0 + 1, &k('w'), "Wait/Defend", color::LIGHTER_GREY);

        let Some(d) = cd.selected_direction else { return };
        let (hx1, hy1) = cd.player_unit_location;
        let (hx2, hy2) = adjacent_cd(hx1, hy1, d);
        if !cd.on_map((hx2, hy2)) {
            return;
        }
        c.fg = color::WHITE;
        c.print_ex(0, 16, Align::Left, "Travel Time:");
        let mut t = self.check_travel((hx1, hy1), (hx2, hy2));
        if !t.is_empty() {
            c.fg = color::LIGHT_RED;
        } else {
            t = format!("{} mins.", self.travel_time((hx1, hy1), (hx2, hy2)));
        }
        c.print_ex(0, 17, Align::Left, &t);
        let z = cd.hex((hx2, hy2));
        if z.controlled_by == 1 {
            c.fg = color::RED;
            c.print_ex(3, 12, Align::Left, "Destination is");
            c.print_ex(3, 13, Align::Left, "Enemy Controlled");
            if !z.known_to_player && !t.contains("N/A") {
                c.fg = color::WHITE;
                c.print_ex(0, 15, Align::Left, "Recon: 10 mins.");
                key_line(c, 3, 10, y0 + 2, &k('r'), "Recon", color::LIGHTER_GREY);
            } else if z.known_to_player {
                c.print_ex(3, 14, Align::Left, &format!("Strength: {}", z.enemy_strength));
            }
        }
        if t.contains("N/A") {
            return;
        }
        key_line(c, 3, 10, y0 + 3, "Enter", "Proceed", color::LIGHTER_GREY);
        if z.controlled_by == 1 {
            c.fg = color::WHITE;
            c.print_ex(1, 19, Align::Left, "Movement Options");
            c.fg = ACTION_KEY_COL;
            c.print_ex(1, 20, Align::Left, &k('t'));
            c.print_ex(1, 21, Align::Left, &k('g'));
            c.print_ex(1, 22, Align::Left, &k('b'));
            c.fg = if cd.advancing_fire { color::WHITE } else { color::DARK_GREY };
            c.print_ex(3, 20, Align::Left, "Advancing Fire");
            c.fg = if !week.contains_key("air_support_level") || cd.weather.cloud_cover == "Overcast" {
                color::DARKEST_GREY
            } else if cd.air_support_request {
                color::WHITE
            } else {
                color::DARK_GREY
            };
            c.print_ex(3, 21, Align::Left, "Request Air Support");
            c.fg = if !week.contains_key("arty_support_level") {
                color::DARKER_GREY
            } else if cd.arty_support_request {
                color::WHITE
            } else {
                color::DARK_GREY
            };
            c.print_ex(3, 22, Align::Left, "Request Arty Support");
        }
    }

    /// The right column: `DisplayTimeInfo`, `DisplayWeatherInfo`,
    /// `UpdateCDCampaignCon`, `UpdateCDHexInfoCon`.
    fn draw_day_right(&self, root: &mut Console) {
        let x0 = sw() - RIGHT_W + 1;
        let w = RIGHT_W - 2;
        let cd = self.cd();
        let camp = self.camp();
        let xm = x0 + w / 2;

        // date and the day's time
        root.fg = color::WHITE;
        root.print_ex(xm, 1, Align::Center, &date_text(&camp.today));
        root.bg = color::DARKER_YELLOW;
        root.rect(x0, 2, w, 1, true);
        let start = text(&camp.week()["day_start"]);
        let mut p = start.split(':').map(|s| s.parse::<i32>().unwrap_or(0));
        let (sh_, sm) = (p.next().unwrap_or(0), p.next().unwrap_or(0));
        let minutes = (cd.day_clock.0 - sh_) * 60 + cd.day_clock.1 - sm;
        let filled = (w as f64 * minutes as f64 / cd.day_length.max(1) as f64) as i32;
        root.bg = color::DARK_YELLOW;
        root.rect(x0, 2, filled.clamp(0, w), 1, true);
        root.bg = color::BLACK;
        root.print_ex(xm, 2, Align::Center, &format!("{:02}:{:02}", cd.day_clock.0, cd.day_clock.1));
        root.fg = color::WHITE;
        root.hline(x0, 3, w);

        self.draw_weather(root, x0, 4, w);
        root.fg = color::WHITE;
        root.hline(x0, 11, w);

        // the day's mission and VP
        root.bg = color::DARKEST_BLUE;
        root.rect(x0, 12, w, 2, true);
        root.fg = color::LIGHT_BLUE;
        root.print_ex(x0, 12, Align::Left, " Day Mission");
        root.print_ex(x0, 13, Align::Left, " VP Today");
        root.fg = color::WHITE;
        root.print_ex(x0 + w - 2, 12, Align::Right, &cd.mission);
        root.print_ex(x0 + w - 2, 13, Align::Right, &cd.day_vp.to_string());
        root.bg = color::BLACK;
        root.fg = color::WHITE;
        root.hline(x0, 14, w);

        self.draw_zone_info(root, x0, 15, w);
    }

    /// `DisplayWeatherInfo`, `w` wide and 7 rows.
    fn draw_weather(&self, root: &mut Console, x0: i32, y0: i32, w: i32) {
        let cd = self.cd();
        let wx = &cd.weather;
        let mut g = Rng::new(self.day_anim.weather_seed.max(1));
        root.bg = color::DARK_BLUE;
        root.rect(x0, y0, w, 2, true);
        let n = match wx.cloud_cover.as_str() {
            "Scattered" => w * 8 / 18,
            "Heavy" => w * 12 / 18,
            "Overcast" => w,
            _ => 0,
        };
        for row in 0..2 {
            let mut cells: Vec<i32> = (0..w).collect();
            shuffle(&mut cells, &mut g);
            for &x in cells.iter().take(n as usize) {
                root.set_char_bg(x0 + x, y0 + row, color::DARK_GREY);
            }
        }
        root.fg = color::WHITE;
        root.print_ex(x0 + w / 2, y0 + 1, Align::Center, &wx.cloud_cover);
        let (ch, fg, bg) = match wx.precipitation.as_str() {
            "Rain" | "Heavy Rain" => (250, color::LIGHT_BLUE, color::DARK_BLUE),
            "Mist" | "Light Snow" | "Snow" | "Blizzard" => (249, color::LIGHT_GREY, color::DARK_GREY),
            _ => (0, color::WHITE, color::DARK_BLUE),
        };
        root.bg = bg;
        root.fg = fg;
        root.rect(x0, y0 + 2, w, 3, true);
        let drops = match wx.precipitation.as_str() {
            "Rain" | "Light Snow" => 9,
            "Heavy Rain" | "Snow" => 14,
            "Blizzard" => 17,
            _ => 0,
        } * w
            / 18;
        for _ in 0..drops {
            let (x, y) = (g.int(0, w - 1), g.int(0, 2));
            root.put_char(x0 + x, y0 + 2 + y, ch);
        }
        if wx.precipitation != "None" {
            root.print_ex(x0 + w / 2, y0 + 3, Align::Center, &wx.precipitation);
        }
        let (fg, bg) = match wx.ground.as_str() {
            "Muddy" => (color::GREY, color::DARKER_SEPIA),
            "Snow" | "Deep Snow" => (color::LIGHT_BLUE, color::GREY),
            _ => (color::WHITE, color::DARK_SEPIA),
        };
        root.fg = fg;
        root.bg = bg;
        root.rect(x0, y0 + 5, w, 2, true);
        root.print_ex(x0 + w / 2, y0 + 5, Align::Center, &wx.ground);
        root.print_ex(x0 + w / 2, y0 + 6, Align::Center, &cd.hex(cd.player_unit_location).terrain_type);
        root.bg = color::BLACK;
    }

    /// The zone the zone info is about: under the mouse if it is on the
    /// map (as in the original), else the chosen direction's, else the
    /// player's own (the original said "Mouseover an area for info").
    fn info_zone(&self) -> Result<(i32, i32), bool> {
        let cd = self.cd();
        if let Some(map) = &self.day_map {
            let (mx, my) = (self.scr.mouse.cx - map.layout.map_x, self.scr.mouse.cy);
            if self.mouse_seen {
                if map.bridges.contains(&(mx, my)) {
                    return Err(true);
                }
                if let Some(z) = map.owner_at(mx, my) {
                    return Ok(z);
                }
            }
        }
        if cd.active_menu == 3 {
            if let Some(d) = cd.selected_direction {
                let n = adjacent_cd(cd.player_unit_location.0, cd.player_unit_location.1, d);
                if cd.on_map(n) {
                    return Ok(n);
                }
            }
        }
        Ok(cd.player_unit_location)
    }

    fn draw_zone_info(&self, root: &mut Console, x0: i32, y0: i32, w: i32) {
        let cd = self.cd();
        root.fg = color::BLUE;
        root.print_ex(x0 + w / 2, y0, Align::Center, "Zone Info");
        let zone = match self.info_zone() {
            Err(_) => {
                root.fg = color::DARK_SEPIA;
                root.print_ex(x0 + 2, y0 + 2, Align::Left, "Bridge");
                return;
            }
            Ok(z) => z,
        };
        let z = cd.hex(zone);
        root.fg = color::LIGHT_GREEN;
        root.print_ex(x0 + w / 2, y0 + 1, Align::Center, &z.coordinate);
        root.fg = color::LIGHT_GREY;
        root.print_ex(x0, y0 + 2, Align::Left, &z.terrain_type);
        if zone == cd.player_unit_location {
            root.print_ex(x0 + w - 1, y0 + 2, Align::Right, "(you are here)");
        }
        if z.controlled_by == 0 {
            root.fg = ALLIED_UNIT_COL;
            root.print_ex(x0, y0 + 4, Align::Left, "Friendly controlled");
        } else {
            root.fg = ENEMY_UNIT_COL;
            root.print_ex(x0, y0 + 4, Align::Left, "Enemy controlled");
            let s = if z.known_to_player { z.enemy_strength.to_string() } else { "Unknown".into() };
            root.print_ex(x0, y0 + 5, Align::Left, &format!("Strength: {s}"));
            root.fg = color::LIGHT_BLUE;
            root.print_ex(x0, y0 + 6, Align::Left, &format!("Capture VP Value: {}", z.vp_value));
        }
        if let Some(too) = z.target_of_opportunity {
            root.fg = color::YELLOW;
            root.print_ex(x0, y0 + 8, Align::Left, "Target of Opportunity");
            root.fg = color::LIGHT_GREY;
            root.print_ex(x0, y0 + 9, Align::Left, &z.too_text);
            root.print_ex(x0, y0 + 10, Align::Left, &format!("VP Bonus: {too}"));
        }
        if z.road_links.contains(&Some(false)) {
            root.fg = DIRT_ROAD_COL;
            root.print_ex(x0, y0 + 12, Align::Left, "Dirt road");
        }
        if z.road_links.contains(&Some(true)) {
            root.fg = STONE_ROAD_COL;
            root.print_ex(x0, y0 + 13, Align::Left, "Stone road");
        }
    }

    /// `ShowMessage` with `cd_highlight`: the zone's outline flashes and
    /// the message is put near it, not over it.
    pub fn day_message(&mut self, text: &str, zone: Option<(i32, i32)>) {
        let anchor = zone.and_then(|z| {
            let map = self.day_map.as_ref()?;
            let (x, y) = map.layout.plot(z.0, z.1);
            Some((map.layout.map_x + x, y, z.1))
        });
        self.day_anim.highlight = zone;
        self.show_message_at(text, false, None, anchor.map(|(x, y, hy)| (x, y, hy <= 4)));
        self.day_anim.highlight = None;
        self.draw_day();
    }

    // ---------------------------------------------------------------------
    // the rules of the day

    /// `CheckTravel`: "" if the move is possible, else why not.
    pub fn check_travel(&self, from: (i32, i32), to: (i32, i32)) -> String {
        let cd = self.cd();
        let Some(d) = direction_to_adjacent_cd(from.0, from.1, to.0, to.1) else { return "N/A".into() };
        let (a, b) = (cd.hex(from), cd.hex(to));
        if a.rivers.contains(&d) && !a.bridges.contains(&d) {
            return "N/A: River crossing".into();
        }
        let back = constrain_dir(d + 3);
        if b.rivers.contains(&back) && !b.bridges.contains(&back) {
            return "N/A: River crossing".into();
        }
        match self.camp().pu().crewman_in("Driver") {
            None => "N/A: No Driver".into(),
            Some(c) if !c.alive => "N/A: Driver is dead".into(),
            _ => String::new(),
        }
    }

    /// `CalculateTravelTime`, in minutes.
    pub fn travel_time(&self, from: (i32, i32), to: (i32, i32)) -> i32 {
        let cd = self.cd();
        let d = direction_to_adjacent_cd(from.0, from.1, to.0, to.1).unwrap_or(0);
        let mut mins = match cd.hex(from).road_links[d as usize] {
            Some(false) => 15,
            Some(true) => 10,
            None if cd.hex(to).terrain_type == "Forest" => 45,
            None => 30,
        };
        match cd.weather.ground.as_str() {
            "Deep Snow" => mins += 20,
            "Muddy" | "Snow" => mins += 10,
            _ if cd.weather.precipitation != "None" => mins += 5,
            _ => {}
        }
        if cd.hex(to).controlled_by == 1 {
            if cd.arty_support_request {
                mins += 15;
            }
            if cd.air_support_request {
                mins += 10;
            }
        }
        if self.camp().check_national_skill("Blitzkrieg") {
            mins = (mins - 10).max(10);
        }
        mins
    }

    /// `AwardVP`: the campaign's and the day's, and experience for the
    /// crew.
    pub fn award_vp(&mut self, vp: i32) {
        self.campm().award_vp(vp);
        self.cdm().day_vp += vp;
    }

    /// `AddRecord`, for the day and the campaign.
    pub fn add_record(&mut self, name: &str, i: i32) {
        self.cdm().add_record(name, i);
        *self.campm().records.entry(name.to_string()).or_insert(0) += i;
    }

    /// `CaptureMe`
    pub fn capture_zone(&mut self, zone: (i32, i32), player: i32, no_vp: bool) {
        if player == 1 {
            self.cdm().hexm(zone).controlled_by = 1;
            return;
        }
        if !no_vp {
            let z = self.cd().hex(zone).clone();
            if z.controlled_by == 1 {
                self.add_record("Map Areas Captured", 1);
                self.add_journal("Captured an enemy-held zone");
                self.award_vp(z.vp_value);
            }
            if let Some(too) = z.target_of_opportunity {
                self.award_vp(too);
                self.day_message("You have captured a Target of Opportunity!", Some(zone));
                self.add_journal("Captured a Target of Opportunity");
            }
        }
        let mission = self.cd().mission.clone();
        let week = self.camp().week().clone();
        let mut rng = std::mem::replace(&mut self.rng, Rng::new(1));
        let z = self.cdm().hexm(zone);
        z.target_of_opportunity = None;
        z.set_enemy_strength(&mission, &week, &mut rng);
        z.controlled_by = 0;
        self.rng = rng;
    }

    /// Every zone's `CalcCaptureVP`.
    pub fn recalc_capture_vp(&mut self) {
        let defensive = self.camp().check_national_skill("Defensive Strategy");
        for h in cd_hexes() {
            self.cdm().calc_capture_vp(h, defensive);
        }
    }

    /// `AdvanceClock`, with the end of the day and the weather checked.
    pub fn advance_clock(&mut self, hours: i32, minutes: i32, skip_checks: bool) {
        let cd = self.cdm();
        cd.day_clock.0 += hours;
        cd.day_clock.1 += minutes;
        while cd.day_clock.1 >= 60 {
            cd.day_clock.0 += 1;
            cd.day_clock.1 -= 60;
        }
        if skip_checks {
            return;
        }
        self.check_for_end_of_day();
        let cd = self.cdm();
        cd.weather_update_clock -= hours * 60 + minutes;
        if cd.weather_update_clock <= 0 {
            self.update_weather();
        }
    }

    /// `CheckForEndOfDay`
    pub fn check_for_end_of_day(&mut self) {
        let cd = self.cdm();
        if cd.day_clock.0 > cd.end_of_day.0 || (cd.day_clock.0 == cd.end_of_day.0 && cd.day_clock.1 >= cd.end_of_day.1) {
            cd.ended = true;
        }
    }

    /// `UpdateWeather`
    pub fn update_weather(&mut self) {
        let gc = num(engine("GROUND_CONDITION_CHANGE_CHANCE"));
        let heavy = num(engine("HEAVY_PRECEP_MOD"));
        let clock = int(engine("BASE_WEATHER_UPDATE_CLOCK")) + self.rng.int(1, 16);
        self.cdm().weather_update_clock = clock;
        let mut roll = percentile(&mut self.rng);
        let wx = self.cd().weather.clone();
        let p = wx.precipitation.as_str();
        let mut ground: Option<(&str, &str)> = None;
        match wx.ground.as_str() {
            "Muddy" => {
                if p == "None" && roll <= gc {
                    ground = Some(("Wet", "The muddy ground has dried out a little."));
                }
            }
            "Wet" => {
                if p == "None" {
                    if roll <= gc {
                        ground = Some(("Dry", "The ground has completely dried out."));
                    }
                } else if ["Rain", "Heavy Rain"].contains(&p) {
                    if p == "Heavy Rain" {
                        roll -= heavy;
                    }
                    if roll <= gc {
                        ground = Some(("Muddy", "The ground has become muddy."));
                    }
                }
            }
            "Snow" => {
                if ["Snow", "Blizzard"].contains(&p) {
                    if p == "Blizzard" {
                        roll -= heavy;
                    }
                    if roll <= gc {
                        ground = Some(("Deep Snow", "The snow has accumilated and is now deep."));
                    }
                }
            }
            _ => {
                if ["Rain", "Heavy Rain"].contains(&p) {
                    if p == "Heavy Rain" {
                        roll -= heavy;
                    }
                    if roll <= gc {
                        ground = Some(("Wet", "The ground has become wet."));
                    }
                } else if ["Snow", "Blizzard"].contains(&p) {
                    if p == "Blizzard" {
                        roll -= heavy;
                    }
                    if roll <= gc {
                        ground = Some(("Snow", "The ground is now covered in snow."));
                    }
                }
            }
        }
        if let Some((g, msg)) = ground {
            self.cdm().weather.ground = g.to_string();
            if self.day_map.is_some() {
                self.rebuild_day_map();
            }
            self.weather_message(msg);
        }

        if percentile(&mut self.rng) > 15.0 {
            return;
        }
        let roll = percentile(&mut self.rng);
        if roll <= 70.0 {
            if self.cd().weather.cloud_cover == "Clear" {
                return;
            }
            let roll = percentile(&mut self.rng);
            let freezing = self.cd().weather.freezing;
            let (new, msg) = match self.cd().weather.precipitation.as_str() {
                "None" if freezing => {
                    if roll <= 80.0 { ("Light Snow", "Light snow begins to fall.") } else { ("Snow", "Snow begins to fall.") }
                }
                "None" => {
                    if roll <= 50.0 {
                        ("Mist", "A light mist begins to fall.")
                    } else if roll <= 85.0 {
                        ("Rain", "Rain begins to fall.")
                    } else {
                        ("Heavy Rain", "A heavy downpour suddenly begins to fall.")
                    }
                }
                "Mist" => {
                    if roll <= 40.0 {
                        ("None", "The light mist has cleared up.")
                    } else if roll <= 80.0 {
                        ("Rain", "The light mist thickens into a steady rain.")
                    } else {
                        ("Heavy Rain", "The light mist suddenly turns into a heavy downpour.")
                    }
                }
                "Rain" => {
                    if roll <= 30.0 {
                        ("None", "The rain has cleared up.")
                    } else if roll <= 80.0 {
                        ("Mist", "The rain turns into a light mist.")
                    } else {
                        ("Heavy Rain", "The rain gets heavier.")
                    }
                }
                "Heavy Rain" => {
                    if roll <= 35.0 { ("Mist", "The rain turns into a light mist.") } else { ("Rain", "The rain lightens a little.") }
                }
                "Light Snow" => {
                    if roll <= 30.0 {
                        ("None", "The snow stops falling.")
                    } else if roll <= 80.0 {
                        ("Snow", "The snow gets a little heavier.")
                    } else {
                        ("Blizzard", "The snow starts falling very heavily.")
                    }
                }
                "Snow" => {
                    if roll <= 20.0 {
                        ("None", "The snow stops falling.")
                    } else if roll <= 80.0 {
                        ("Light Snow", "The snow gets a little lighter.")
                    } else {
                        ("Blizzard", "The snow starts falling very heavily.")
                    }
                }
                _ => {
                    if roll <= 20.0 {
                        ("Light Snow", "The snow gets quite a bit lighter.")
                    } else {
                        ("Snow", "The snow gets a bit lighter.")
                    }
                }
            };
            self.cdm().weather.precipitation = new.to_string();
            self.weather_message(msg);
        } else {
            let roll = percentile(&mut self.rng);
            let (new, msg) = match self.cd().weather.cloud_cover.as_str() {
                "Clear" => {
                    if roll <= 85.0 { ("Scattered", "Scattered clouds begin to form.") } else { ("Heavy", "A heavy cloud front rolls in.") }
                }
                "Scattered" => {
                    if roll <= 75.0 {
                        ("Clear", "The clouds part and the sky is clear.")
                    } else if roll <= 90.0 {
                        ("Heavy", "The cloud cover gets thicker.")
                    } else {
                        ("Overcast", "A storm front has rolled in.")
                    }
                }
                "Heavy" => {
                    if roll <= 85.0 { ("Scattered", "The clouds begin to thin out.") } else { ("Overcast", "The cloud cover thickens.") }
                }
                _ => ("Heavy", "The cloud cover begins to thin out but remains heavy."),
            };
            self.cdm().weather.cloud_cover = new.to_string();
            self.weather_message(msg);
            if new == "Clear" && self.cd().weather.precipitation != "None" {
                self.cdm().weather.precipitation = "None".into();
            }
        }
        if self.day_map.is_some() {
            self.init_day_animations();
        }
    }

    fn weather_message(&mut self, msg: &str) {
        if self.day_map.is_some() {
            self.init_day_animations();
            self.day_message(msg, None);
        } else {
            self.show_message(msg, false, None);
        }
    }

    /// `MovePlayerTo`: the marker moves across, the clock goes on.
    pub fn move_player_to(&mut self, to: (i32, i32)) {
        let from = self.cd().player_unit_location;
        let mins = self.travel_time(from, to);
        self.cdm().selected_direction = None;
        let pu = self.camp().pu().clone();
        crate::sound::play_movement(&pu, &mut self.rng);
        if let Some(map) = &self.day_map {
            let (x1, y1) = map.layout.plot(from.0, from.1);
            let (x2, y2) = map.layout.plot(to.0, to.1);
            for (x, y) in get_line(0, 0, x2 - x1, y2 - y1, false) {
                self.day_anim.offset = (x, y);
                self.draw_day();
                self.flush();
                self.wait(12, false);
            }
            self.day_anim.offset = (0, 0);
        }
        self.advance_clock(0, mins, false);
        self.cdm().player_unit_location = to;
    }

    /// `CheckForCDMapShift`: at the far edge, a new map region.
    pub fn check_for_map_shift(&mut self) {
        let (px, py) = self.cd().player_unit_location;
        let mission = self.cd().mission.clone();
        let edge = if ["Fighting Withdrawal", "Counterattack"].contains(&mission.as_str()) { 8 } else { 0 };
        if py != edge {
            return;
        }
        self.day_message("You enter a new map region.", None);
        self.add_journal("Entered a new map region");
        let (new_hy, hx_mod) = if py == 0 { (8, -4) } else { (0, 4) };
        self.cdm().maps_traversed += if py == 0 { 1 } else { -1 };
        let all = cd_hexes();
        let week = self.camp().week().clone();
        let region = engine("REGIONS")[text(&self.camp().stats["region"])].as_object().expect("region").clone();
        let mut rng = std::mem::replace(&mut self.rng, Rng::new(1));
        let cd = self.cdm();
        // the player's row becomes the new map's far row, roads and rivers
        // and all (which now face other zones); the rest is new, without
        // roads or rivers, as the original does not make them again
        let mut moved: Vec<CdMapHex> = Vec::new();
        for hx in px - 4..=px + 4 {
            if all.contains(&(hx, py)) {
                let mut z = cd.map_hexes[&(hx, py)].clone();
                z.hx = hx + hx_mod;
                z.hy = new_hy;
                z.coordinate = format!("{}{}", (b'A' as i32 + new_hy) as u8 as char, 5 + z.hx);
                moved.push(z);
            }
        }
        cd.player_unit_location = (px + hx_mod, new_hy);
        let mut hexes: BTreeMap<(i32, i32), CdMapHex> = BTreeMap::new();
        for z in moved {
            hexes.insert((z.hx, z.hy), z);
        }
        for &(hx, hy) in &all {
            if hy == new_hy {
                continue;
            }
            let mut z = CdMapHex::new(hx, hy, &mission, &week, &mut rng);
            z.generate_terrain_type(&region, &week, &mut rng);
            hexes.insert((hx, hy), z);
        }
        cd.map_hexes = hexes;
        self.rng = rng;
        self.recalc_capture_vp();
        let loc = self.cd().player_unit_location;
        let cd = self.cdm();
        for z in cd.map_hexes.values_mut() {
            z.controlled_by = if mission == "Fighting Withdrawal" { 0 } else { 1 };
        }
        cd.hexm(loc).controlled_by = 0;
        if self.day_map.is_some() {
            self.rebuild_day_map();
        }
        crate::save::save_game(self);
    }

    /// `CheckForRandomEvent`
    pub fn check_for_random_event(&mut self) {
        if self.cd().ended || self.scenario.is_some() {
            return;
        }
        let roll = percentile(&mut self.rng);
        if roll > self.cd().random_event_chance {
            self.cdm().random_event_chance += 2.0;
            return;
        }
        let roll = percentile(&mut self.rng);
        if roll <= 20.0 {
            // a new target of opportunity
            if self.cd().mission == "Fighting Withdrawal" {
                return;
            }
            let cd = self.cd();
            if cd.map_hexes.values().filter(|z| z.target_of_opportunity.is_some()).count() >= 2 {
                return;
            }
            let (px, py) = cd.player_unit_location;
            let mut list: Vec<(i32, (i32, i32))> = Vec::new();
            for hy in 0..=py {
                for hx in px - 4..=px + 4 {
                    let Some(z) = cd.map_hexes.get(&(hx, hy)) else { continue };
                    if z.target_of_opportunity.is_some() || z.controlled_by == 0 {
                        continue;
                    }
                    list.push((z.enemy_strength, (hx, hy)));
                }
            }
            let Some(highest) = list.iter().map(|l| l.0).max() else { return };
            list.retain(|l| l.0 >= highest);
            let (_, zone) = *self.rng.choice(&list);
            let extra = self.rng.int(2, 5);
            let roll = percentile(&mut self.rng);
            let t = if roll <= 20.0 {
                "Ammo Depot"
            } else if roll <= 40.0 {
                "Supply Line"
            } else if roll <= 60.0 {
                "Communications Centre"
            } else if roll <= 80.0 {
                "Enemy Intel"
            } else {
                "Fuel Dump"
            };
            let z = self.cdm().hexm(zone);
            z.target_of_opportunity = Some(10);
            z.enemy_strength = (z.enemy_strength + extra).min(10);
            z.too_text = t.to_string();
            self.day_message(
                &format!("We have received word of a new target of opportunity ({t}), capture this zone if possible."),
                Some(zone),
            );
        } else if roll <= 30.0 {
            let list: Vec<(i32, i32)> = self.cd().map_hexes.iter().filter(|(_, z)| z.controlled_by != 0).map(|(k, _)| *k).collect();
            if list.is_empty() {
                return;
            }
            let zone = *self.rng.choice(&list);
            let extra = self.rng.int(1, 3);
            let z = self.cdm().hexm(zone);
            z.enemy_strength = (z.enemy_strength + extra).min(10);
            // as in the original, an unknown zone's change is not shown and
            // the event chance is not reset
            if !z.known_to_player {
                return;
            }
            self.day_message("We have reports of an increase of enemy strength in a zone!", Some(zone));
        } else if roll <= 40.0 {
            let list: Vec<(i32, i32)> = self
                .cd()
                .map_hexes
                .iter()
                .filter(|(_, z)| z.controlled_by != 0 && !z.known_to_player)
                .map(|(k, _)| *k)
                .collect();
            if list.is_empty() {
                return;
            }
            let zone = *self.rng.choice(&list);
            self.cdm().hexm(zone).known_to_player = true;
            self.day_message("We have received information about expected enemy strength in an area.", Some(zone));
        } else if roll <= 60.0 {
            let list: Vec<(i32, i32)> = self
                .cd()
                .map_hexes
                .iter()
                .filter(|(_, z)| z.controlled_by != 0 && z.known_to_player)
                .map(|(k, _)| *k)
                .collect();
            if list.is_empty() {
                return;
            }
            let zone = *self.rng.choice(&list);
            self.cdm().hexm(zone).known_to_player = false;
            self.day_message("Enemy movement reported in a map zone, estimated strength no longer certain.", Some(zone));
            let change = self.rng.int(0, 6);
            let z = self.cdm().hexm(zone);
            z.enemy_strength = (z.enemy_strength - 3 + change).clamp(1, 10);
        } else if roll <= 75.0 {
            let week = self.camp().week().clone();
            let more = 10.0 * self.rng.int(1, 3) as f64;
            let what = if let Some(max) = week.get("air_support_level") {
                let cd = self.cdm();
                cd.air_support_level = (cd.air_support_level + more).min(num(max));
                "air"
            } else if let Some(max) = week.get("arty_support_level") {
                let cd = self.cdm();
                cd.arty_support_level = (cd.arty_support_level + more).min(num(max));
                "artillery"
            } else {
                return;
            };
            self.day_message(&format!("Additional {what} support has made available to you from Command."), None);
        } else if roll <= 80.0 {
            for p in &mut self.campm().pum().positions {
                if let Some(c) = &mut p.crewman {
                    if c.alive {
                        c.fatigue = (c.fatigue - c.stat("Morale") / 2).max(-5);
                    }
                }
            }
            self.day_message("Your crew feel a little less fatigued than before.", None);
        } else {
            return;
        }
        self.cdm().random_event_chance = num(engine("BASE_CD_RANDOM_EVENT_CHANCE"));
    }

    /// `CheckForZoneCapture`: allied and enemy forces take zones.
    pub fn check_for_zone_capture(&mut self, zone_just_captured: bool) {
        if self.cd().ended {
            return;
        }
        let (friendly, enemy, enemy_max) = match self.cd().mission.as_str() {
            "Advance" | "Spearhead" => (45.0, 5.0, 1),
            "Battle" => (20.0, 20.0, 2),
            "Fighting Withdrawal" => (3.0, 85.0, 4),
            "Counterattack" => (5.0, 80.0, 2),
            _ => return,
        };
        // the original looks for a neighbour in five of the six directions
        // (`range(5)`), kept
        let has_neighbour = |cd: &crate::cday::CampaignDay, (hx, hy): (i32, i32), side: i32| {
            (0..5).any(|d| {
                let n = adjacent_cd(hx, hy, d);
                cd.on_map(n) && cd.hex(n).controlled_by == side
            })
        };
        if percentile(&mut self.rng) <= friendly {
            let cd = self.cd();
            let list: Vec<(i32, i32)> = cd
                .map_hexes
                .iter()
                .filter(|(k, z)| z.controlled_by != 0 && (k.1 == 8 || has_neighbour(cd, **k, 0)))
                .map(|(k, _)| *k)
                .collect();
            if !list.is_empty() {
                let zone = *self.rng.choice(&list);
                self.day_message("Allied forces have captured an enemy-held zone!", Some(zone));
                self.capture_zone(zone, 0, true);
            }
        }
        if percentile(&mut self.rng) <= enemy {
            let cd = self.cd();
            let player = cd.player_unit_location;
            let list: Vec<(i32, i32)> = cd
                .map_hexes
                .iter()
                .filter(|(k, z)| z.controlled_by != 1 && !(zone_just_captured && **k == player) && (k.1 == 0 || has_neighbour(cd, **k, 1)))
                .map(|(k, _)| *k)
                .collect();
            if !list.is_empty() {
                let n = self.rng.int(1, enemy_max) as usize;
                let mut captured = list.clone();
                if n < list.len() {
                    shuffle(&mut captured, &mut self.rng);
                    captured.truncate(n);
                }
                for &z in &captured {
                    self.capture_zone(z, 1, false);
                }
                if captured.contains(&player) {
                    self.day_message("Enemy forces attack your zone!", None);
                    self.add_journal("Enemy forces attacked our zone");
                    self.scenario = Some(crate::battle::Scenario::new(player));
                    self.add_record("Battles Fought", 1);
                } else if captured.len() > 1 {
                    self.day_message("Enemy forces have captured allied-held zones!", None);
                } else {
                    self.day_message("Enemy forces have captured an allied-held zone!", Some(captured[0]));
                }
            }
        }
    }

    /// `DisplayCampaignDaySummary`
    pub fn display_day_summary(&mut self) {
        let camp = self.camp();
        let cd = self.cd();
        let (w, h) = (40, 22);
        let mut t = new_console(w, h, color::BLACK, color::LIGHT_GREY);
        draw_frame(&mut t, 0, 0, w, h);
        t.fg = PORTRAIT_BG_COL;
        for (i, line) in wrap(camp.name(), (w - 2) as usize, "").iter().take(2).enumerate() {
            t.print_name(w / 2, 1 + i as i32, Align::Center, line);
        }
        t.fg = color::WHITE;
        t.print_ex(w / 2, 4, Align::Center, &date_text(&camp.today));
        t.print_ex(w / 2, 6, Align::Center, "Outcome of Day:");
        let (col, outcome) = if camp.player_oob {
            (color::RED, "COMMANDER OUT OF ACTION")
        } else if cd.abandoned_tank {
            (color::LIGHT_GREY, "ABANDONED TANK")
        } else if camp.pu().immobilized {
            (color::LIGHT_RED, "IMMOBILIZED")
        } else if camp.pu().alive {
            (GOLD_COL, "SURVIVED")
        } else {
            (ENEMY_UNIT_COL, "TANK LOST")
        };
        t.fg = col;
        t.print_ex(w / 2, 7, Align::Center, outcome);
        t.fg = color::WHITE;
        t.print_ex(w / 2, 9, Align::Center, "VP Earned Today:");
        t.print_ex(w / 2, 10, Align::Center, &cd.day_vp.to_string());
        let mut y = 12;
        for r in defs::strings(engine("RECORD_LIST")) {
            t.print_ex(4, y, Align::Left, &format!("{r}:"));
            t.print_ex(w - 5, y, Align::Right, &cd.records.get(&r).copied().unwrap_or(0).to_string());
            y += 1;
        }
        key_line(&mut t, w / 2 - 7, w / 2, h - 2, "Enter", "Continue", color::LIGHT_GREY);
        self.root.darken(0.7);
        Console::blit(&t, 0, 0, w, h, &mut self.root, sxm() - w / 2, sh() / 2 - h / 2);
        if self.scr.is_headless() && self.script.is_empty() {
            return;
        }
        loop {
            self.flush();
            if self.input() && matches!(self.key().vk, Vk::Escape | Vk::Enter) {
                return;
            }
        }
    }

    /// What follows a battle in `DoCampaignDayLoop`.
    fn after_battle(&mut self) {
        self.do_crew_recovery_check();
        if self.camp().options.permadeath {
            let dead = self.camp().pu().positions.first().and_then(|p| p.crewman.as_ref()).is_some_and(|c| !c.alive);
            if dead {
                self.cdm().ended = true;
                let camp = self.campm();
                camp.ended = true;
                camp.player_oob = true;
                return;
            }
        }
        if self.camp().pu().immobilized || !self.camp().pu().alive {
            self.display_day_summary();
            self.cdm().ended = true;
            return;
        }
        if self.cd().player_withdrew {
            let from = self.cd().player_unit_location;
            let list: Vec<(i32, i32)> = (0..6)
                .map(|d| adjacent_cd(from.0, from.1, d))
                .filter(|&n| self.cd().on_map(n) && self.cd().hex(n).controlled_by != 1 && self.check_travel(from, n).is_empty())
                .collect();
            if list.is_empty() {
                self.day_message("ERROR: Could not find an adjacent friendly hex zone!", None);
            } else {
                let to = *self.rng.choice(&list);
                self.move_player_to(to);
                self.check_for_map_shift();
            }
            self.cdm().player_withdrew = false;
        } else {
            let here = self.cd().player_unit_location;
            self.capture_zone(here, 0, false);
        }
        if self.camp().pu().bogged {
            self.day_message("You free your tank from being bogged down.", None);
            self.campm().pum().bogged = false;
        }
        let mut freed = Vec::new();
        for w in &mut self.campm().pum().weapons {
            if w.jammed {
                w.jammed = false;
                freed.push(format!("{} is no longer jammed.", w.name()));
            }
        }
        for m in freed {
            self.day_message(&m, None);
        }
        let mut rng = std::mem::replace(&mut self.rng, Rng::new(1));
        for p in &mut self.campm().pum().positions {
            if let Some(c) = &mut p.crewman {
                c.fatigue_check(&mut rng);
            }
        }
        self.rng = rng;
        self.check_for_end_of_day();
        if !self.cd().ended {
            self.check_for_random_event();
            self.check_for_zone_capture(true);
            self.recalc_capture_vp();
            self.check_for_map_shift();
        }
        crate::save::save_game(self);
    }

    /// `DoCampaignDayLoop`, until the day ends or the player saves and
    /// quits.
    pub fn do_campaign_day_loop(&mut self) {
        self.rebuild_day_map();
        self.init_day_animations();
        self.draw_day();
        if !self.cd().travel_time_spent {
            let minutes = 5 + self.rng.int(1, 3) * 10;
            self.advance_clock(0, minutes, true);
            self.draw_day();
            self.day_message(&format!("It takes you {minutes} minutes to travel to the front lines."), None);
            self.add_journal("Arrived at front lines");
            self.cdm().travel_time_spent = true;
        }
        crate::save::save_game(self);
        let mut mouse = (-1, -1);
        loop {
            if self.camp().player_oob {
                self.display_day_summary();
                break;
            }
            if self.scenario.is_some() {
                self.do_scenario_loop();
                if self.exiting {
                    break;
                }
                if self.scenario.is_none() {
                    self.after_battle();
                }
                self.draw_day();
                continue;
            }
            if self.cd().ended {
                self.day_message("Your combat day has ended.", None);
                self.add_journal("End of day");
                self.display_day_summary();
                break;
            }
            self.day_tick();
            self.flush();
            if self.take_redraw() {
                self.draw_day();
            }
            if self.scr.is_headless() && self.script.is_empty() {
                self.day_autopilot();
            }
            let pressed = self.input();
            let m = (self.scr.mouse.cx, self.scr.mouse.cy);
            if m != mouse {
                if mouse != (-1, -1) || m != (0, 0) {
                    self.mouse_seen = true;
                }
                mouse = m;
                self.draw_day();
            }
            if !pressed {
                continue;
            }
            if self.key().vk == Vk::Escape {
                self.show_game_menu();
                if self.exiting {
                    break;
                }
                self.draw_day();
                continue;
            }
            let k = self.mapped_char();
            if ('1'..='4').contains(&k) {
                let n = k as i32 - '0' as i32;
                if self.cd().active_menu != n {
                    self.cdm().active_menu = n;
                    self.draw_day();
                }
                continue;
            }
            match self.cd().active_menu {
                1 => self.day_supply_key(k),
                2 => self.day_crew_key(k),
                3 => self.day_travel_key(k),
                _ => {}
            }
            self.draw_day();
        }
        self.day_map = None;
        self.msg = None;
    }

    fn day_supply_key(&mut self, k: char) {
        if let Some(g) = self.cd().selected_gun {
            match k {
                'c' => {
                    self.campm().pum().weapons[g].cycle_ammo();
                    return;
                }
                'a' | 'd' => {
                    self.campm().pum().weapons[g].manage_rr(if k == 'a' { -1 } else { 1 });
                    return;
                }
                'q' => {
                    let list = self.cd().gun_list.clone();
                    if list.len() > 1 {
                        let i = list.iter().position(|&x| x == g).unwrap_or(0);
                        self.cdm().selected_gun = Some(list[(i + 1) % list.len()]);
                    }
                    return;
                }
                _ => {}
            }
        }
        if k != 'r' {
            return;
        }
        let (hx1, hy1) = self.cd().player_unit_location;
        let path_good = hy1 == 8 || (-4..=0).any(|hx2| !self.cd().hex_path((hx1, hy1), (hx2, 8), true, true).is_empty());
        if !path_good {
            self.day_message("You are cut off from friendly forces! No resupply possible.", None);
            return;
        }
        if !self.show_notification("Spend 30 minutes waiting for resupply?", true) {
            return;
        }
        self.day_message("You contact HQ for resupply, which arrives 30 minutes later.", None);
        self.advance_clock(0, 30, false);
        self.add_journal("We are resupplied");
        self.ammo_reload_menu();
        self.draw_day();
        // the original also tried to update battle screens here, which a
        // campaign day does not have, and crashed when a dead crewman was
        // taken back
        let mut rng = std::mem::replace(&mut self.rng, Rng::new(1));
        let mut taken = Vec::new();
        for p in &mut self.campm().pum().positions {
            match &mut p.crewman {
                Some(c) if !c.alive => {
                    taken.push(format!("The body of your {} is taken back by the supply team.", p.name));
                    p.crewman = None;
                }
                Some(c) => c.rest(&mut rng),
                None => {}
            }
        }
        self.rng = rng;
        for m in taken {
            self.day_message(&m, None);
        }
        if (self.cd().player_squad.len() as i32) < self.camp().player_squad_max {
            let mut rng = std::mem::replace(&mut self.rng, Rng::new(1));
            let camp = self.campaign.take().unwrap();
            crate::cday::spawn_player_squad(&camp, self.cdm(), &mut rng);
            self.campaign = Some(camp);
            self.rng = rng;
            self.day_message("You are joined by reserve units, bringing your squad back up to full strength.", None);
        }
        self.check_for_zone_capture(false);
        self.check_for_random_event();
        crate::save::save_game(self);
    }

    fn day_crew_key(&mut self, k: char) {
        let n = self.camp().pu().positions.len();
        let sel = self.cd().selected_position;
        match k {
            'w' => self.cdm().selected_position = (sel + n - 1) % n,
            's' => self.cdm().selected_position = (sel + 1) % n,
            'f' => {
                if self.camp().pu().positions[sel].crewman.is_some() {
                    self.day_map_paused(|g| g.show_crewman_menu(CrewRef::Position(sel)));
                }
            }
            _ => match self.key_char() {
                'p' => self.day_map_paused(|g| g.show_swap_position_menu()),
                'h' => {
                    if self.campm().pum().toggle_hatch(sel) {
                        crate::sound::play("hatch");
                    }
                }
                _ => {}
            },
        }
    }

    /// Open a page screen over the day: the day map stops drawing itself
    /// while it is up.
    fn day_map_paused(&mut self, f: impl FnOnce(&mut Game)) {
        let map = self.day_map.take();
        f(self);
        self.day_map = map;
    }

    fn day_travel_key(&mut self, k: char) {
        const DIRECTION_KEYS: [char; 6] = ['e', 'd', 'c', 'z', 'a', 'q'];
        if let Some(d) = DIRECTION_KEYS.iter().position(|&x| x == k) {
            let d = d as i32;
            let cd = self.cdm();
            cd.selected_direction = if cd.selected_direction == Some(d) { None } else { Some(d) };
            return;
        }
        if k == 'w' {
            if self.show_notification("Remain in place for 15 minutes?", true) {
                self.day_message("You remain in place, ready for possible attack.", None);
                self.cdm().selected_direction = None;
                self.advance_clock(0, 15, false);
                self.check_for_random_event();
                self.check_for_zone_capture(false);
                crate::save::save_game(self);
            }
            return;
        }
        if k == 'i' {
            if self.show_notification("Abandon your tank? You will forfeit half of today's VP.", true) {
                let half = self.cd().day_vp / 2;
                let camp = self.campm();
                camp.pum().alive = false;
                camp.player_vp -= half;
                self.day_message("You abandon your tank.", None);
                self.add_journal("Abandoned tank");
                let cd = self.cdm();
                cd.ended = true;
                cd.abandoned_tank = true;
            }
            return;
        }
        let Some(d) = self.cd().selected_direction else { return };
        let from = self.cd().player_unit_location;
        let to = adjacent_cd(from.0, from.1, d);
        if !self.cd().on_map(to) || !self.check_travel(from, to).is_empty() {
            return;
        }
        let enemy = self.cd().hex(to).controlled_by == 1;
        let week = self.camp().week().clone();
        match k {
            't' if enemy => {
                let cd = self.cdm();
                cd.advancing_fire = !cd.advancing_fire;
                crate::save::save_game(self);
            }
            'g' if enemy && week.contains_key("air_support_level") && self.cd().weather.cloud_cover != "Overcast" => {
                let cd = self.cdm();
                cd.air_support_request = !cd.air_support_request;
                crate::save::save_game(self);
            }
            'b' if enemy && week.contains_key("arty_support_level") => {
                let cd = self.cdm();
                cd.arty_support_request = !cd.arty_support_request;
                crate::save::save_game(self);
            }
            'r' => {
                let z = self.cd().hex(to);
                if z.known_to_player || z.controlled_by == 0 {
                    return;
                }
                self.cdm().hexm(to).known_to_player = true;
                let s = self.cd().hex(to).enemy_strength;
                self.day_message(&format!("Estimated enemy strength in zone: {s}"), Some(to));
                self.advance_clock(0, 10, false);
                self.check_for_random_event();
                crate::save::save_game(self);
            }
            _ if self.key().vk == Vk::Enter => self.travel_to(to),
            _ => {}
        }
    }

    /// Enter on the travel tab: move, and in enemy ground, perhaps meet
    /// resistance.
    fn travel_to(&mut self, to: (i32, i32)) {
        self.move_player_to(to);
        self.draw_day();
        if self.cd().hex(to).controlled_by == 1 {
            let mut text = "You enter the enemy-held zone".to_string();
            if self.cd().advancing_fire {
                let used = self.rng.int(4, 10);
                let w = &mut self.campm().pum().weapons[0];
                let mut done = false;
                if w.stat("type") == Some("Gun") {
                    if let Some(he) = w.ammo_stores.as_mut().and_then(|s| s.get_mut("HE")) {
                        if *he > 0 {
                            *he = (*he - used).max(0);
                            done = true;
                        }
                    }
                }
                if done {
                    text += ", firing at anything suspicious";
                } else {
                    self.cdm().advancing_fire = false;
                }
            }
            self.day_message(&format!("{text}."), None);
            let roll = percentile(&mut self.rng) - self.cd().encounter_mod;
            if roll <= self.cd().hex(to).enemy_strength as f64 * 9.5 {
                self.day_message("You encounter enemy resistance!", None);
                self.advance_clock(0, 15, false);
                self.scenario = Some(crate::battle::Scenario::new(to));
                self.cdm().encounter_mod = 0.0;
                self.add_record("Battles Fought", 1);
                self.add_journal("Entered an enemy-held zone, encountered resistance");
                crate::save::save_game(self);
                return;
            }
            self.day_message("You find no resistance and gain control of the area.", None);
            self.add_journal("Entered an enemy-held zone, no resistance");
            self.capture_zone(to, 0, false);
            self.cdm().encounter_mod += 30.0;
            for air in [true, false] {
                let cd = self.cd();
                let (req, level) = if air { (cd.air_support_request, cd.air_support_level) } else { (cd.arty_support_request, cd.arty_support_level) };
                if !req {
                    continue;
                }
                let mut level = level;
                if percentile(&mut self.rng) <= level {
                    level -= 10.0;
                }
                let cd = self.cdm();
                if air {
                    cd.air_support_level = level.max(0.0);
                    cd.air_support_request = false;
                } else {
                    cd.arty_support_level = level.max(0.0);
                    cd.arty_support_request = false;
                }
            }
        } else {
            self.day_message("You enter the allied-held zone.", None);
        }
        self.recalc_capture_vp();
        self.check_for_map_shift();
        self.draw_day();
        self.check_for_zone_capture(true);
        self.check_for_random_event();
        crate::save::save_game(self);
    }

    /// Headless runs: a player who heads for the far edge, recons and
    /// waits now and then.
    fn day_autopilot(&mut self) {
        let cd = self.cd();
        let from = cd.player_unit_location;
        let goal_row = if ["Fighting Withdrawal", "Counterattack"].contains(&cd.mission.as_str()) { 8 } else { 0 };
        let mut options: Vec<i32> = (0..6)
            .filter(|&d| {
                let n = adjacent_cd(from.0, from.1, d);
                cd.on_map(n) && self.check_travel(from, n).is_empty()
            })
            .collect();
        shuffle(&mut options, &mut self.rng);
        options.sort_by_key(|&d| (adjacent_cd(from.0, from.1, d).1 - goal_row).abs());
        let roll = self.rng.int(1, 20);
        let keys: Vec<char> = if options.is_empty() || roll == 1 {
            vec!['3', 'w', 'y']
        } else if roll == 2 {
            vec!['1', 'r', 'y']
        } else if roll == 3 {
            vec!['2', 'h']
        } else {
            let d = options[0];
            let key = ['e', 'd', 'c', 'z', 'a', 'q'][d as usize];
            let mut k = vec!['3', key];
            if self.cd().selected_direction == Some(d) {
                k.pop();
            }
            if roll == 4 {
                k.push('r');
            }
            k.push('\r');
            k
        };
        self.script.extend(keys.into_iter().map(press));
    }
}

/// Python's `str()` of a float: `80.0`, `75.5`.
fn py_float(v: f64) -> String {
    format!("{v:?}")
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::cday::tests::new_game;

    #[test]
    fn zones_tile_the_map() {
        for (w, h) in [(100, 30), (140, 45), (200, 60)] {
            let g = new_game("poland_campaign", 3);
            let map = build_day_map_in(g.cd(), DayLayout::for_screen(w, h));
            let l = map.layout;
            assert!(l.map_x + l.map_w <= w);
            // every zone's centre is its own, and on the map
            for (&(hx, hy), _) in &g.cd().map_hexes {
                let (x, y) = l.plot(hx, hy);
                assert_eq!(map.owner_at(x, y), Some((hx, hy)), "{w}x{h}");
                assert!(x >= 0 && x < l.map_w && y >= 0 && y < l.map_h);
            }
            // every zone has cells, about the same number
            let mut counts: BTreeMap<(i32, i32), i32> = BTreeMap::new();
            for o in map.owner.iter().flatten() {
                *counts.entry(*o).or_insert(0) += 1;
            }
            assert_eq!(counts.len(), 41);
            let (min, max) = (counts.values().min().unwrap(), counts.values().max().unwrap());
            assert!(max - min <= 4, "{w}x{h}: {min}..{max}");
            // every neighbouring pair shares an edge
            for (&z, _) in &g.cd().map_hexes {
                for d in 0..6 {
                    if g.cd().on_map(adjacent_cd(z.0, z.1, d)) {
                        assert!(!map.edge_cells(z, d).is_empty(), "{z:?} {d}");
                    }
                }
            }
        }
    }

    #[test]
    fn shot() {
        let Ok(dir) = std::env::var("ARMCOM2_SHOTS") else { return };
        for (i, (name, _)) in defs::defs().campaigns.iter().enumerate().take(4) {
            let mut g = new_game(name, 11 + i as u64);
            g.cdm().active_menu = 3;
            g.cdm().selected_direction = Some(5);
            g.rebuild_day_map();
            g.init_day_animations();
            g.draw_day();
            let mut s = String::new();
            for y in 0..g.root.h {
                for x in 0..g.root.w {
                    s.push(crate::glyph::to_char(g.root.char_at(x, y)));
                }
                s.push('\n');
            }
            std::fs::write(format!("{dir}/day_{i}.txt"), s).unwrap();
        }
        let g = new_game("pattons_best", 5);
        for (w, h) in [(140, 45), (200, 60)] {
            let map = build_day_map_in(g.cd(), DayLayout::for_screen(w, h));
            let mut s = String::new();
            for y in 0..map.con.h {
                for x in 0..map.con.w {
                    s.push(crate::glyph::to_char(map.con.char_at(x, y)));
                }
                s.push('\n');
            }
            std::fs::write(format!("{dir}/daymap_{w}x{h}.txt"), s).unwrap();
        }
    }

    #[test]
    fn days_play_to_their_end() {
        for seed in 1..=30u64 {
            let name = &defs::defs().campaigns[seed as usize % 10].0;
            let mut g = new_game(name, seed);
            g.ammo_reload_menu();
            g.cdm().started = true;
            g.do_campaign_day_loop();
            assert!(g.cd().ended || g.camp().ended, "{name} {seed}");
        }
    }
}
