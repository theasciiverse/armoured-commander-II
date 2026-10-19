//! The battle: the original's `Scenario` class and the battle half of
//! `Unit`, `Weapon` and `Personnel` (spotting, line of sight, attacks,
//! armour penetration, firepower, morale, injuries, movement, the phases
//! of a turn, random events, air and artillery support).
//!
//! The original's units point at each other (line of sight tables,
//! acquired targets, the player's squad); here a battle keeps its units in
//! one list that only grows (`units`), refers to them by their index, and
//! keeps the order the original's `scenario.units` list has, which units
//! leave, in `order`. The player's tank and squad are copied in when the
//! battle starts and back when it ends, as the original does.
//!
//! Directions and bearings are worked out on the original's square-cell
//! hex plot (`PlotHex`), so facings come out as they did; only the drawing
//! (`battle_ui.rs`) uses the terminal's hexes.

use crate::defs::{self, engine, int, num, text};
use crate::game::*;
use crate::hexes::{self, constrain_dir, distance, ring};
use crate::model::{CrewContext, Personnel, Unit, percentile};
use crate::rng::Rng;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PHASE_COMMAND: i32 = 0;
pub const PHASE_SPOTTING: i32 = 1;
pub const PHASE_CREW_ACTION: i32 = 2;
pub const PHASE_MOVEMENT: i32 = 3;
pub const PHASE_SHOOTING: i32 = 4;
pub const PHASE_ALLIED_ACTION: i32 = 5;
pub const PHASE_ENEMY_ACTION: i32 = 6;

/// An attack or penetration profile (the original's `profile` dict).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub attacker: usize,
    pub weapon: usize,
    pub ammo_type: Option<String>,
    pub target: usize,
    pub result: String,
    /// "Point Fire", "Area Fire", "Close Combat" or "ap".
    pub kind: String,
    /// The position of the crewman firing, if any.
    pub crewman: Option<usize>,
    pub critical_hit: f64,
    pub base_chance: f64,
    pub base_fp: i32,
    pub effective_fp: i32,
    pub modifier_list: Vec<(String, f64)>,
    pub final_chance: f64,
    pub full_effect: f64,
    pub critical_effect: f64,
    pub location: Option<String>,
    pub location_desc: String,
    pub final_score: i32,
    pub roll: f64,
}

/// What `Personnel.ResolveAttack` returns: nothing (`None`), no change
/// (`False`), or a description of the result.
#[derive(Clone, Debug, PartialEq)]
pub enum CrewHit {
    None,
    False,
    Text(String),
}

impl CrewHit {
    pub fn is_none(&self) -> bool {
        matches!(self, CrewHit::None)
    }
}

/// What a crewman is hit by, for `ResolveAttack`.
#[derive(Clone, Debug, Default)]
pub struct CrewAttack {
    pub firepower: Option<i32>,
    pub spalling: bool,
    /// The knock-out hit on the tank: where it hit, and whether it was a
    /// close combat weapon.
    pub ko_hit: Option<(Option<String>, bool)>,
    pub burn_up: bool,
    pub sniper: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Scenario {
    pub init_complete: bool,
    /// The day map zone this battle is in.
    pub zone: (i32, i32),
    pub ambush: bool,
    pub finished: bool,
    pub enemy_nation: Option<String>,
    pub class_type_dict: BTreeMap<String, String>,
    pub random_event_chance: f64,
    pub enemy_reinforcements: i32,
    /// The map's hexes, the player's and four rings around it, in the
    /// original's order, and the units in each, top first.
    pub hexes: Vec<(i32, i32)>,
    pub stacks: Vec<Vec<usize>>,
    /// Every unit that has been in this battle.
    pub units: Vec<Unit>,
    /// The units in play, in the original's list order.
    pub order: Vec<usize>,
    pub current_turn: i32,
    pub active_player: i32,
    pub phase: i32,
    pub advance_phase: bool,
    pub context_text: String,
    pub player_pivot: i32,
    pub target_list: Vec<usize>,
    pub selected_weapon: Option<usize>,
    pub selected_position: usize,
    pub player: usize,
    pub squad: Vec<usize>,
}

impl Scenario {
    pub fn new(zone: (i32, i32)) -> Self {
        let mut hexes = Vec::new();
        for r in 0..5 {
            hexes.extend(ring(0, 0, r));
        }
        let stacks = vec![Vec::new(); hexes.len()];
        Scenario {
            init_complete: false,
            zone,
            ambush: false,
            finished: false,
            enemy_nation: None,
            class_type_dict: BTreeMap::new(),
            random_event_chance: num(engine("BASE_RANDOM_EVENT_CHANCE")),
            enemy_reinforcements: 0,
            hexes,
            stacks,
            units: Vec::new(),
            order: Vec::new(),
            current_turn: 1,
            active_player: 0,
            phase: PHASE_COMMAND,
            advance_phase: false,
            context_text: String::new(),
            player_pivot: 0,
            target_list: Vec::new(),
            selected_weapon: None,
            selected_position: 0,
            player: 0,
            squad: Vec::new(),
        }
    }

    pub fn on_map(&self, h: (i32, i32)) -> bool {
        self.hexes.contains(&h)
    }

    pub fn stack(&self, h: (i32, i32)) -> &Vec<usize> {
        let i = self.hexes.iter().position(|&x| x == h).expect("a map hex");
        &self.stacks[i]
    }

    pub fn stack_mut(&mut self, h: (i32, i32)) -> &mut Vec<usize> {
        let i = self.hexes.iter().position(|&x| x == h).expect("a map hex");
        &mut self.stacks[i]
    }

    pub fn is_player_side(&self, i: usize) -> bool {
        i == self.player || self.squad.contains(&i)
    }

    /// `los_table[u2]` of unit `u1` (false if never rolled).
    pub fn los(&self, u1: usize, u2: usize) -> bool {
        self.units[u1].battle.los.get(&u2).copied().unwrap_or(false)
    }
}

/// `PlotHex`: a hex's place on the original's square-cell map, used for
/// bearings.
pub fn plot_sq(hx: i32, hy: i32) -> (i32, i32) {
    (hx * 7 + 26, hy * 6 + hx * 3 + 21)
}

/// `GetBearing`
pub fn bearing(x1: i32, y1: i32, x2: i32, y2: i32) -> i32 {
    let deg = ((y2 - y1) as f64).atan2((x2 - x1) as f64).to_degrees() + 90.0;
    deg.rem_euclid(360.0) as i32
}

/// `RectifyBearing`
pub fn rectify(h: i32) -> i32 {
    h.rem_euclid(360)
}

/// `GetDirectionToward`
pub fn direction_toward(hx1: i32, hy1: i32, hx2: i32, hy2: i32) -> i32 {
    let (x1, y1) = plot_sq(hx1, hy1);
    let (x2, y2) = plot_sq(hx2, hy2);
    let b = bearing(x1, y1, x2, y2);
    if b >= 330 || b <= 30 {
        0
    } else if b <= 90 {
        1
    } else if b >= 270 {
        5
    } else if b <= 150 {
        2
    } else if b >= 210 {
        4
    } else {
        3
    }
}

/// `RestrictChance`
pub fn restrict(chance: f64) -> f64 {
    round1(chance).clamp(3.0, 97.0)
}

/// `round(x, 1)`
pub fn round1(x: f64) -> f64 {
    (x * 10.0).round_ties_even() / 10.0
}

/// `Get2D6Odds`
pub fn odds_2d6(score: i32) -> f64 {
    match score {
        2 => 2.7,
        3 => 8.3,
        4 => 16.7,
        5 => 27.8,
        6 => 41.8,
        7 => 58.3,
        8 => 72.2,
        9 => 83.3,
        10 => 91.7,
        _ => 97.2,
    }
}

fn mg_weapons() -> Vec<String> {
    defs::strings(engine("MG_WEAPONS"))
}

fn player_positions() -> Vec<String> {
    defs::strings(engine("PLAYER_POSITIONS"))
}

fn terrain_effects(t: &str) -> Option<&'static serde_json::Map<String, serde_json::Value>> {
    engine("SCENARIO_TERRAIN_EFFECTS").get(t).and_then(|v| v.as_object())
}

impl Game {
    /// The next unit of a Python `for u in reversed(scenario.units)` loop,
    /// which keeps an index into a list units leave while it runs (a unit
    /// that destroys one before it in the list acts again, as in the
    /// original).
    pub fn rev_next(&self, idx: &mut isize) -> Option<usize> {
        let order = &self.scenario.as_ref()?.order;
        if *idx < 0 || *idx as usize >= order.len() {
            return None;
        }
        let u = order[*idx as usize];
        *idx -= 1;
        Some(u)
    }

    pub fn sc(&self) -> &Scenario {
        self.scenario.as_ref().expect("a battle")
    }

    pub fn scm(&mut self) -> &mut Scenario {
        self.scenario.as_mut().expect("a battle")
    }

    pub fn su(&self, i: usize) -> &Unit {
        &self.sc().units[i]
    }

    pub fn sum(&mut self, i: usize) -> &mut Unit {
        &mut self.scm().units[i]
    }

    pub fn pl(&self) -> usize {
        self.sc().player
    }

    /// The player's unit: the battle's copy during a battle.
    pub fn player_unit(&self) -> &Unit {
        match &self.scenario {
            Some(s) => &s.units[s.player],
            None => self.camp().pu(),
        }
    }

    pub fn player_unit_mut(&mut self) -> &mut Unit {
        if self.scenario.is_some() {
            let s = self.scenario.as_mut().unwrap();
            let p = s.player;
            return &mut s.units[p];
        }
        self.campm().pum()
    }

    fn with_rng<T>(&mut self, f: impl FnOnce(&mut Game, &mut Rng) -> T) -> T {
        let mut rng = std::mem::replace(&mut self.rng, Rng::new(1));
        let r = f(self, &mut rng);
        self.rng = rng;
        r
    }

    fn roll(&mut self) -> f64 {
        percentile(&mut self.rng)
    }

    /// A unit's name as the player sees it (`GetName`).
    pub fn uname(&self, i: usize) -> String {
        self.su(i).name().to_string()
    }

    /// A message about a battle hex (`ShowMessage(scenario_highlight=)`).
    pub fn bmsg(&mut self, text: &str, hex: Option<(i32, i32)>) {
        self.battle_message(text, false, None, hex);
    }

    // ---------------------------------------------------------------------
    // setting units down

    /// `SpawnAt`
    pub fn spawn_at(&mut self, i: usize, hx: i32, hy: i32) {
        let s = self.scm();
        s.units[i].hx = hx;
        s.units[i].hy = hy;
        s.order.push(i);
        s.stack_mut((hx, hy)).push(i);
        self.generate_terrain(i);
        self.check_for_hd(i, false);
        self.set_smoke_level(i);
    }

    /// Add a unit to the battle; its index.
    pub fn add_unit(&mut self, u: Unit) -> usize {
        let s = self.scm();
        s.units.push(u);
        s.units.len() - 1
    }

    /// `GenerateTerrain`
    pub fn generate_terrain(&mut self, i: usize) {
        let seed = self.rng.int(1, 128);
        let zone = self.sc().zone;
        let cd_terrain = self.cd().hex(zone).terrain_type.clone();
        let odds = engine("SCENARIO_TERRAIN_ODDS")[&cd_terrain].as_object().expect("terrain odds");
        let mut roll = self.roll();
        let mut terrain = odds.keys().next().cloned();
        for (t, v) in odds {
            if roll <= num(v) {
                terrain = Some(t.clone());
                break;
            }
            roll -= num(v);
        }
        let u = self.sum(i);
        u.battle.terrain_seed = seed;
        u.terrain = terrain;
    }

    /// `CheckForHD`
    pub fn check_for_hd(&mut self, i: usize, driver_attempt: bool) -> bool {
        let u = self.su(i);
        if ["Infantry", "Gun"].contains(&u.category()) {
            return false;
        }
        self.sum(i).hull_down.clear();
        let u = self.su(i);
        let Some(effects) = u.terrain.as_deref().and_then(terrain_effects) else { return false };
        let mut chance = num(&effects["HD Chance"]);
        if let Some(size) = u.stat("size_class") {
            chance += engine("HD_SIZE_MOD").get(size).map(num).unwrap_or(0.0);
        }
        if driver_attempt {
            if let Some(d) = u.position_of("Driver") {
                let c = u.positions[d].crewman.as_ref().unwrap();
                let m = if self.untrained_position(i, d) {
                    -20.0
                } else {
                    let m = c.skill_mod(6.0);
                    if c.ce { m } else { m * 0.5 }
                };
                chance += m;
            }
            for pos in ["Commander", "Commander/Gunner"] {
                let Some(c) = u.crewman_in(pos) else { continue };
                if c.current_cmd == "Direct Movement" {
                    let m = c.skill_mod(6.0);
                    chance += if c.ce { m } else { m * 0.5 };
                    if c.skills.iter().any(|s| s == "Lay of the Land") {
                        chance += c.skill_mod(15.0);
                    }
                }
                break;
            }
        } else if let Some(d) = u.position_of("Driver") {
            let c = u.positions[d].crewman.as_ref().unwrap();
            if c.skills.iter().any(|s| s == "Eye for Cover") && !self.untrained_position(i, d) {
                chance += c.skill_mod(5.0);
            }
        }
        let chance = restrict(chance);
        if self.roll() > chance {
            return false;
        }
        let dir = if driver_attempt { self.su(i).facing.unwrap_or(0) } else { self.rng.int(0, 5) };
        self.sum(i).hull_down = vec![dir, constrain_dir(dir + 1), constrain_dir(dir - 1)];
        true
    }

    /// `SetSmokeLevel`
    pub fn set_smoke_level(&mut self, i: usize) {
        let mut roll = self.roll();
        match self.cd().weather.precipitation.as_str() {
            "Rain" => roll -= 2.5,
            "Heavy Rain" => roll -= 4.0,
            _ => {}
        }
        self.sum(i).smoke = if roll <= 97.0 {
            0
        } else if roll <= 99.0 {
            1
        } else {
            2
        };
    }

    /// `GetTEM`
    pub fn tem(&self, i: usize) -> f64 {
        let u = self.su(i);
        let Some(effects) = u.terrain.as_deref().and_then(terrain_effects) else { return 0.0 };
        let Some(tem) = effects.get("TEM").and_then(|t| t.as_object()) else { return 0.0 };
        if let Some(v) = tem.get("All") {
            return num(v);
        }
        match u.category() {
            "Vehicle" => tem.get("Vehicle").map(num).unwrap_or(0.0),
            "Infantry" => tem.get("Infantry").map(num).unwrap_or(0.0),
            "Gun" if u.battle.deployed => tem.get("Deployed Gun").map(num).unwrap_or(0.0),
            _ => 0.0,
        }
    }

    /// `MoveToTopOfStack`
    pub fn move_to_top_of_stack(&mut self, i: usize) {
        let h = (self.su(i).hx, self.su(i).hy);
        let st = self.scm().stack_mut(h);
        if st.len() <= 1 || !st.contains(&i) {
            return;
        }
        st.retain(|&x| x != i);
        st.insert(0, i);
    }

    /// `RemoveFromPlay`
    pub fn remove_from_play(&mut self, i: usize) {
        let h = (self.su(i).hx, self.su(i).hy);
        let s = self.scm();
        s.stack_mut(h).retain(|&x| x != i);
        s.order.retain(|&x| x != i);
    }

    // ---------------------------------------------------------------------
    // crew

    /// `UntrainedPosition`
    pub fn untrained_position(&self, i: usize, p: usize) -> bool {
        let Some(s) = &self.scenario else { return false };
        if i != s.player {
            return false;
        }
        let pos = &s.units[i].positions[p];
        let Some(c) = &pos.crewman else { return false };
        if pos.name == c.normal_position {
            return false;
        }
        let list = engine("POSITION_SWITCH_LIST").get(&c.normal_position).map(defs::strings).unwrap_or_default();
        !list.contains(&pos.name)
    }

    /// `BuildCommandList` for the crewman in position `p` of unit `i`.
    pub fn build_command_list(&mut self, i: usize, p: usize) {
        let u = self.su(i);
        let pos = &u.positions[p];
        let Some(c) = &pos.crewman else { return };
        let mut list: Vec<String> = Vec::new();
        if c.incapacitated() {
            list.push("None".into());
        } else {
            let mg = mg_weapons();
            for (k, d) in &defs::defs().crew_commands {
                if k == "None" {
                    continue;
                }
                if k == "Operate Gun" || k == "Operate MG" {
                    for w in &u.weapons {
                        let t = w.stat("type").unwrap_or("");
                        if k == "Operate Gun" && t != "Gun" {
                            continue;
                        }
                        if k == "Operate MG" && !mg.iter().any(|m| m == t) {
                            continue;
                        }
                        let Some(fb) = defs::get(&w.stats, "fired_by") else { continue };
                        if !defs::strings(fb).contains(&pos.name) {
                            continue;
                        }
                        list.push(k.clone());
                    }
                    continue;
                }
                if k == "Manage Ready Rack" {
                    for w in &u.weapons {
                        if w.stat("type") != Some("Gun") {
                            continue;
                        }
                        let by = |key: &str| defs::get(&w.stats, key).map(defs::strings).unwrap_or_default().contains(&pos.name);
                        if by("fired_by") || by("reloaded_by") {
                            list.push(k.clone());
                        }
                    }
                    continue;
                }
                if let Some(pl) = d.get("position_list") {
                    if !defs::strings(pl).contains(&pos.name) {
                        continue;
                    }
                }
                match k.as_str() {
                    "Abandon Tank" => {
                        let can = u.immobilized
                            || u.positions.iter().filter_map(|p| p.crewman.as_ref()).any(|c| !c.alive || c.condition == "Critical");
                        if !can {
                            continue;
                        }
                    }
                    "Drive" => {
                        if u.immobilized || u.bogged {
                            continue;
                        }
                    }
                    "Overrun" => {
                        if u.immobilized || u.bogged {
                            continue;
                        }
                        if !u.weapons.iter().any(|w| mg.iter().any(|m| Some(m.as_str()) == w.stat("type"))) {
                            continue;
                        }
                        let s = self.sc();
                        let defending = s.on_map((0, -1))
                            && s.stack((0, -1)).iter().any(|&o| {
                                let o = &s.units[o];
                                o.owning_player == 1 && o.spotted && ["Infantry", "Gun"].contains(&o.category())
                            });
                        if !defending {
                            continue;
                        }
                    }
                    "Attempt Unbog" => {
                        if !u.bogged {
                            continue;
                        }
                    }
                    "Withdraw" => {
                        if u.immobilized || u.bogged {
                            continue;
                        }
                        let s = self.sc();
                        let near = s.order.iter().any(|&o| s.units[o].owning_player != 0 && distance(s.units[o].hx, s.units[o].hy, 0, 0) <= 2);
                        if near {
                            continue;
                        }
                    }
                    "Fire Smoke Mortar" => {
                        if u.stat("smoke_mortar") != Some(pos.name.as_str()) {
                            continue;
                        }
                    }
                    _ => {}
                }
                list.push(k.clone());
            }
        }
        if let Some(c) = &mut self.sum(i).positions[p].crewman {
            c.cmd_list = list;
        }
    }

    /// `BuildCmdLists`
    pub fn build_cmd_lists(&mut self, i: usize) {
        for p in 0..self.su(i).positions.len() {
            if self.su(i).positions[p].crewman.is_none() {
                continue;
            }
            self.build_command_list(i, p);
            let c = self.sum(i).positions[p].crewman.as_mut().unwrap();
            if !c.cmd_list.contains(&c.current_cmd) {
                c.current_cmd = c.cmd_list.first().cloned().unwrap_or_else(|| "None".into());
            }
        }
    }

    /// `SelectCommand`
    pub fn select_command(&mut self, i: usize, p: usize, reverse: bool) {
        let Some(c) = &mut self.sum(i).positions[p].crewman else { return };
        if c.cmd_list.is_empty() {
            return;
        }
        let n = c.cmd_list.len();
        let at = c.cmd_list.iter().position(|x| *x == c.current_cmd).unwrap_or(0);
        let next = if reverse { (at + n - 1) % n } else { (at + 1) % n };
        c.current_cmd = c.cmd_list[next].clone();
    }

    /// `Position.UpdateVisibleHexes`
    pub fn update_visible_hexes(&mut self, i: usize, p: usize) {
        let s = self.sc();
        let u = &s.units[i];
        let pos = &u.positions[p];
        let mut list = Vec::new();
        if let Some(c) = &pos.crewman {
            list.push((u.hx, u.hy));
            let allowed = defs::defs().crew_commands.get(&c.current_cmd).and_then(|d| d.get("spotting_allowed")).and_then(|v| v.as_bool()).unwrap_or(false);
            if allowed {
                let dirs = if pos.hatch_open { &pos.ce_visible } else { &pos.bu_visible };
                let rotate = match pos.location.as_deref() {
                    Some("Hull") => u.facing.unwrap_or(0),
                    Some("Turret") => u.turret_facing.unwrap_or(0),
                    _ => 0,
                };
                let max_bu = int(engine("MAX_BU_LOS"));
                for &d in dirs {
                    for h in hexes::covered(u.hx, u.hy, constrain_dir(d + rotate)) {
                        if !s.on_map(h) || list.contains(&h) {
                            continue;
                        }
                        if !pos.hatch_open && distance(u.hx, u.hy, h.0, h.1) > max_bu {
                            continue;
                        }
                        list.push(h);
                    }
                }
            }
        }
        self.sum(i).positions[p].visible_hexes = list;
    }

    /// `Personnel.DoGritCheck`
    fn grit_check(&mut self, c: &Personnel, modifier: f64) -> bool {
        self.roll() + modifier <= c.stat("Grit") as f64 * 9.0
    }

    /// `Personnel.DoMoraleCheck`
    fn morale_check_crew(&mut self, c: &Personnel, modifier: f64) -> bool {
        self.roll() + modifier <= c.stat("Morale") as f64 * 9.0
    }

    /// `Personnel.DoStunCheck`: true if not stunned.
    pub fn stun_check(&mut self, i: usize, p: usize, modifier: f64) -> bool {
        let c = self.su(i).positions[p].crewman.clone().unwrap();
        if !["Good Order", "Shaken"].contains(&c.condition.as_str()) {
            return true;
        }
        self.roll() + modifier <= c.stat("Grit") as f64 * 9.0
    }

    /// The crewman's name in messages: "you" for the player's position.
    fn crew_is_player(&self, i: usize, p: usize) -> bool {
        player_positions().contains(&self.su(i).positions[p].name)
    }

    /// `Personnel.KIA`
    pub fn kia(&mut self, i: usize, p: usize) {
        let is_player_pos = self.crew_is_player(i, p);
        if let Some(c) = &mut self.sum(i).positions[p].crewman {
            c.alive = false;
            c.condition = "Dead".into();
            c.fatigue = 0;
        }
        if self.camp().options.permadeath && is_player_pos {
            if let Some(s) = &mut self.scenario {
                s.finished = true;
            }
            self.cdm().ended = true;
            let camp = self.campm();
            camp.ended = true;
            camp.player_oob = true;
        }
    }

    /// `Personnel.ResolveAttack` for the crewman in position `p` of unit
    /// `i`.
    pub fn resolve_crew_attack(&mut self, i: usize, p: usize, attack: &CrewAttack, show_messages: bool) -> CrewHit {
        let Some(c) = self.su(i).positions[p].crewman.clone() else { return CrewHit::None };
        if !c.alive {
            return CrewHit::None;
        }
        let show = show_messages && i == self.pl();
        let pos_name = self.su(i).positions[p].name.clone();
        let pos_loc = self.su(i).positions[p].location.clone();
        let you = self.crew_is_player(i, p);
        let mut modifier = 0.0;
        if let Some(fp) = attack.firepower {
            if self.su(i).has("armour") {
                if !c.ce || c.incapacitated() && c.alive {
                    return CrewHit::False;
                }
            }
            modifier += match fp {
                ..=2 => -75.0,
                3..=4 => -55.0,
                5..=6 => -35.0,
                7..=8 => 5.0,
                9..=10 => 15.0,
                11..=12 => 20.0,
                _ => 25.0,
            };
        } else if attack.spalling {
            modifier -= self.rng.int(1, 40) as f64;
        } else if let Some((location, close_combat)) = &attack.ko_hit {
            if *close_combat && c.ce {
                modifier += 25.0;
            }
            if let (Some(l), Some(pl)) = (location, &pos_loc) {
                if l == pl {
                    modifier += 20.0;
                }
            }
            // extra ammunition aboard
            for &g in &self.cd().gun_list.clone() {
                let pu = self.su(self.pl());
                let Some(w) = pu.weapons.get(g) else { continue };
                let total = w.total_stores();
                if total > w.max_ammo {
                    if let Some(cal) = defs::get(&w.stats, "calibre") {
                        let per = if int(cal) <= 37 { 1.0 } else { 2.0 };
                        modifier += per * (total - w.max_ammo) as f64;
                    }
                }
            }
        } else if attack.burn_up && !c.ce {
            modifier += self.rng.int(2, 5) as f64 * 10.0;
        }
        modifier -= c.stat("Grit") as f64 * 3.0;
        if c.condition == "Shaken" {
            modifier += 15.0;
        }
        let mut roll = self.roll();
        if roll <= 99.5 {
            roll += modifier;
        }
        let mut lroll = self.roll();
        if attack.sniper {
            lroll -= 21.0;
        }
        let location = if lroll <= 15.0 {
            "Head & Neck"
        } else if lroll <= 50.0 {
            "Torso & Groin"
        } else if lroll <= 65.0 {
            "Right Arm & Hand"
        } else if lroll <= 80.0 {
            "Left Arm & Hand"
        } else if lroll <= 90.0 {
            "Right Leg & Foot"
        } else {
            "Left Leg & Foot"
        };
        if attack.firepower.is_some() && location.contains("Leg") {
            return CrewHit::None;
        }
        let msg = |g: &mut Game, you_text: &str, other_text: String| {
            if show {
                g.bmsg(if you { you_text } else { &other_text }, None);
            }
        };
        let injury;
        if roll < 35.0 {
            return CrewHit::None;
        } else if roll <= 45.0 {
            if c.condition != "Good Order" {
                return CrewHit::False;
            }
            if self.morale_check_crew(&c, 0.0) {
                return CrewHit::False;
            }
            self.crew_mut(i, p).condition = "Shaken".into();
            msg(self, "You had a near miss and are now Shaken.", format!("Your {pos_name} had a near miss and is now Shaken."));
            return CrewHit::Text("Shaken".into());
        } else if roll <= 65.0 {
            // the original compares the condition with a list, which is
            // never equal, so a grazing hit never stuns or shakes (kept)
            msg(
                self,
                "You suffer a grazing hit but are no worse for wear.",
                format!("Your {pos_name} suffers a grazing hit but is no worse for wear."),
            );
            return CrewHit::None;
        } else if roll <= 80.0 {
            let r = self.roll();
            injury = if r <= 50.0 {
                "Light"
            } else if r <= 80.0 {
                "Heavy"
            } else {
                "Serious"
            };
        } else if roll <= 90.0 {
            let r = self.roll();
            injury = if r <= 40.0 {
                "Light"
            } else if r <= 60.0 {
                "Heavy"
            } else if r <= 85.0 {
                "Serious"
            } else {
                "Critical"
            };
        } else if roll <= 98.9 {
            injury = "Critical";
        } else {
            if you && self.cd().fate_points > 0 {
                self.cdm().fate_points -= 1;
                return CrewHit::False;
            }
            self.kia(i, p);
            let text = if self.camp().options.permadeath && you {
                format!("You have been hit in the {location} and killed. Your campaign is over.")
            } else {
                format!("Your {pos_name} has been hit in the {location} and killed.")
            };
            if show {
                self.bmsg(&text, None);
            }
            self.add_journal(&text);
            return CrewHit::Text("KIA".into());
        }
        if ["Serious", "Critical"].contains(&injury) && you && self.cd().fate_points > 0 {
            self.cdm().fate_points -= 1;
            return CrewHit::None;
        }
        let current = c.injury.iter().find(|(k, _)| k == location).and_then(|(_, v)| v.clone());
        let mut change = false;
        let mut new = current.clone();
        match (&current, injury) {
            (None, _) => {
                new = Some(injury.to_string());
                change = true;
            }
            (Some(cur), "Light") => {
                if cur == "Light" && self.roll() <= 50.0 {
                    new = Some("Heavy".into());
                    change = true;
                }
            }
            (Some(cur), "Heavy") => {
                if cur == "Light" {
                    new = Some("Heavy".into());
                    change = true;
                } else if cur == "Heavy" && self.roll() <= 50.0 {
                    new = Some("Serious".into());
                    change = true;
                }
            }
            (Some(cur), "Serious") => {
                if cur == "Light" || cur == "Heavy" {
                    new = Some("Serious".into());
                    change = true;
                } else if cur == "Serious" && self.roll() <= 50.0 {
                    new = Some("Critical".into());
                    change = true;
                }
            }
            (Some(cur), _) => {
                if cur != "Critical" {
                    new = Some("Critical".into());
                    change = true;
                }
            }
        }
        if let Some(slot) = self.crew_mut(i, p).injury.iter_mut().find(|(k, _)| k == location) {
            slot.1 = new;
        }
        let text = if change {
            self.with_rng(|g, rng| g.crew_mut(i, p).fatigue_check(rng));
            if you {
                format!("You were hit in the {location} and suffer a {injury} injury.")
            } else {
                format!("Your {pos_name} was hit in the {location} and suffers a {injury} injury.")
            }
        } else if you {
            format!("You were hit in the {location} but suffer no further injury.")
        } else {
            format!("Your {pos_name} was hit in the {location} but suffers no further injury.")
        };
        if show {
            self.bmsg(&text, None);
        }
        self.add_journal(&text);
        if !change {
            return CrewHit::Text(injury.into());
        }
        let c = self.su(i).positions[p].crewman.clone().unwrap();
        let condition_change = if location == "Head & Neck" {
            if !self.grit_check(&c, 20.0) {
                Some("Stunned")
            } else if !self.grit_check(&c, 0.0) {
                Some("Unconscious")
            } else {
                Some("Shaken")
            }
        } else if location == "Torso & Groin" {
            if !self.grit_check(&c, 15.0) {
                Some("Stunned")
            } else if !self.grit_check(&c, -10.0) {
                Some("Unconscious")
            } else {
                None
            }
        } else {
            None
        };
        let Some(cc) = condition_change else { return CrewHit::Text(injury.into()) };
        let cond = c.condition.as_str();
        if (cc == "Shaken" && cond != "Good Order")
            || (cc == "Stunned" && ["Stunned", "Unconscious"].contains(&cond))
            || (cc == "Unconscious" && cond == "Unconscious")
        {
            return CrewHit::Text(injury.into());
        }
        self.crew_mut(i, p).condition = cc.to_string();
        let text = if you {
            format!("As a result of your injury, you are now {cc}.")
        } else {
            format!("As a result of the injury, your {pos_name} is now {cc}.")
        };
        if show {
            self.bmsg(&text, None);
        }
        self.add_journal(&text);
        CrewHit::Text(format!("{injury}, {cc}"))
    }

    pub fn crew_mut(&mut self, i: usize, p: usize) -> &mut Personnel {
        self.sum(i).positions[p].crewman.as_mut().expect("a crewman")
    }

    /// `Personnel.DoInjuryCheck`, at the start of a unit's turn.
    pub fn do_injury_check(&mut self, i: usize, p: usize) {
        let Some(c) = self.su(i).positions[p].crewman.clone() else { return };
        if !c.alive {
            return;
        }
        let you = self.crew_is_player(i, p);
        let pos_name = self.su(i).positions[p].name.clone();
        for (k, v) in &c.injury {
            if v.as_deref() != Some("Critical") {
                continue;
            }
            let mut roll = self.roll();
            if roll > 97.0 {
                self.kia(i, p);
                // the original named an unset `location` here and crashed
                let text = if self.camp().options.permadeath && you {
                    format!("You succumb to your {k} injury and die. Your campaign is over.")
                } else {
                    format!("Your {pos_name} succumbed to their {k} injury and has died.")
                };
                self.bmsg(&text, None);
                self.add_journal(&text);
                return;
            }
            let first_aid = self
                .su(i)
                .positions
                .iter()
                .enumerate()
                .filter(|(q, _)| *q != p)
                .filter_map(|(_, x)| x.crewman.as_ref())
                .filter(|x| x.current_cmd == "First Aid")
                .count();
            roll -= 15.0 * first_aid as f64;
            if c.condition == "Unconscious" {
                roll += 15.0;
            }
            if roll <= c.stat("Grit") as f64 * 9.0 {
                if let Some(slot) = self.crew_mut(i, p).injury.iter_mut().find(|(x, _)| x == k) {
                    slot.1 = Some("Serious".into());
                }
                let text = if you {
                    format!("Your {k} injury has stabilized and is now Serious.")
                } else {
                    format!("Your {pos_name}'s {k} injury has stabilized and is now Serious.")
                };
                self.bmsg(&text, None);
                self.add_journal(&text);
            }
        }
        let c = self.su(i).positions[p].crewman.clone().unwrap();
        let (who, verb) = if you { ("You".to_string(), "") } else { (format!("Your {pos_name}"), "s") };
        match c.condition.as_str() {
            "Shaken" => {
                if self.morale_check_crew(&c, 0.0) {
                    self.crew_mut(i, p).condition = "Good Order".into();
                    self.bmsg(&format!("{who} recover{verb} from being Shaken."), None);
                }
            }
            "Stunned" => {
                if self.morale_check_crew(&c, 15.0) {
                    self.crew_mut(i, p).condition = "Good Order".into();
                    self.bmsg(&format!("{who} recover{verb} from being Stunned."), None);
                }
            }
            "Unconscious" => {
                if self.grit_check(&c, 25.0) {
                    self.crew_mut(i, p).condition = "Stunned".into();
                    let t = if you {
                        "You regain consciousness and are now Stunned.".to_string()
                    } else {
                        format!("Your {pos_name} regains consciousness and is now Stunned.")
                    };
                    self.bmsg(&t, None);
                }
            }
            _ => {}
        }
    }

    // ---------------------------------------------------------------------
    // weapons

    /// `Weapon.UpdateCoveredHexes`
    pub fn update_covered_hexes(&mut self, i: usize, w: usize) {
        let s = self.sc();
        let u = &s.units[i];
        let wp = &u.weapons[w];
        let mut list = vec![(u.hx, u.hy)];
        let all_around = |list: &mut Vec<(i32, i32)>| {
            for r in 1..=wp.max_range {
                for h in ring(u.hx, u.hy, r) {
                    if s.on_map(h) {
                        list.push(h);
                    }
                }
            }
        };
        if u.category() == "Infantry" || (wp.stat("type") == Some("AA MG") && !wp.stats.contains_key("front_only")) {
            all_around(&mut list);
        } else {
            let dir = if wp.stat("mount") == Some("Hull") || u.turret_facing.is_none() {
                u.facing
            } else if wp.stat("mount") == Some("Turret") {
                u.turret_facing
            } else {
                None
            };
            if let Some(d) = dir {
                for h in hexes::covered(u.hx, u.hy, d) {
                    if s.on_map(h) && distance(u.hx, u.hy, h.0, h.1) <= wp.max_range {
                        list.push(h);
                    }
                }
            }
        }
        self.sum(i).weapons[w].covered_hexes = list;
    }

    pub fn update_all_covered_hexes(&mut self, i: usize) {
        for w in 0..self.su(i).weapons.len() {
            self.update_covered_hexes(i, w);
        }
    }

    /// `Weapon.JamTest`
    fn jam_test(&mut self, i: usize, w: usize) -> bool {
        if self.su(i).weapons[w].jammed {
            return false;
        }
        if self.roll() > num(engine("WEAPON_JAM_CHANCE")) {
            return false;
        }
        self.sum(i).weapons[w].jammed = true;
        true
    }

    /// `Weapon.AttemptUnjam`
    fn attempt_unjam(&mut self, i: usize, w: usize) -> bool {
        let u = self.su(i);
        let wp = &u.weapons[w];
        if !wp.jammed {
            return false;
        }
        let Some(fb) = defs::get(&wp.stats, "fired_by").map(defs::strings) else { return false };
        let t = wp.stat("type").unwrap_or("");
        let req = if t == "Gun" {
            "Operate Gun"
        } else if mg_weapons().iter().any(|m| m == t) {
            "Operate MG"
        } else {
            return false;
        };
        if !fb.iter().any(|pos| u.crewman_in(pos).is_some_and(|c| c.current_cmd == req)) {
            return false;
        }
        if self.roll() > num(engine("WEAPON_UNJAM_CHANCE")) {
            return false;
        }
        self.sum(i).weapons[w].jammed = false;
        true
    }

    /// `Weapon.BreakdownTest`
    fn weapon_breakdown_test(&mut self, i: usize, w: usize) -> bool {
        let chance = if defs::get(&self.su(i).weapons[w].stats, "unreliable").is_some() { 1.0 } else { 0.3 };
        if self.roll() > chance {
            return false;
        }
        self.sum(i).weapons[w].broken = true;
        true
    }

    /// `Weapon.GetRoFChance`
    pub fn rof_chance(&self, i: usize, w: usize) -> f64 {
        let u = self.su(i);
        let wp = &u.weapons[w];
        let is_gun = wp.stat("type") == Some("Gun");
        let is_mg = mg_weapons().iter().any(|m| Some(m.as_str()) == wp.stat("type"));
        if is_gun && u.moving {
            return 0.0;
        }
        let mut chance = defs::get(&wp.stats, "rof").map(num).unwrap_or(5.0);
        if is_gun && !wp.using_rr {
            chance = (chance * 0.5 * 100.0).round_ties_even() / 100.0;
        }
        let mut modifier = 0.0;
        if is_gun {
            if let Some(t) = &wp.ammo_type {
                let have = if wp.using_rr {
                    wp.ready_rack.get(t).copied().unwrap_or(0)
                } else {
                    wp.ammo_stores.as_ref().and_then(|s| s.get(t).copied()).unwrap_or(0)
                };
                if have == 0 {
                    return 0.0;
                }
            }
            for pos in defs::get(&wp.stats, "reloaded_by").map(defs::strings).unwrap_or_default() {
                let Some(c) = u.crewman_in(&pos) else { continue };
                if c.current_cmd != "Reload" {
                    continue;
                }
                modifier = 20.0;
                if c.skills.iter().any(|s| s == "Fast Hands") {
                    modifier += c.skill_mod(15.0);
                }
                break;
            }
            if modifier == 0.0 {
                modifier = -15.0;
            }
        }
        for pos in defs::get(&wp.stats, "fired_by").map(defs::strings).unwrap_or_default() {
            let Some(c) = u.crewman_in(&pos) else { continue };
            let has = |s: &str| c.skills.iter().any(|x| x == s);
            if is_gun {
                if has("Quick Trigger") {
                    modifier += c.skill_mod(5.0);
                }
                if wp.selected_target.is_some() && has("Time on Target") {
                    modifier += c.skill_mod(10.0);
                }
            } else if is_mg && has("Burst Fire") {
                modifier += c.skill_mod(10.0);
            }
        }
        if i == self.pl() && is_mg && self.camp().check_national_skill("Superior Firepower") {
            modifier += 25.0;
        }
        (chance + modifier).max(0.0)
    }

    /// `Weapon.AddAcquiredTarget`
    fn add_acquired_target(&mut self, i: usize, w: usize, target: usize) {
        let t = self.su(target);
        if t.owning_player == 1 && !t.spotted {
            return;
        }
        let wp = &mut self.sum(i).weapons[w];
        wp.acquired_target = match wp.acquired_target {
            Some((x, 0)) if x == target => Some((target, 1)),
            Some((x, 1)) if x == target => Some((target, 1)),
            _ => Some((target, 0)),
        };
    }

    // ---------------------------------------------------------------------
    // units

    /// `ClearAcquiredTargets`
    pub fn clear_acquired_targets(&mut self, i: usize, no_enemy: bool) {
        for w in &mut self.sum(i).weapons {
            w.acquired_target = None;
        }
        if no_enemy {
            return;
        }
        let side = self.su(i).owning_player;
        let s = self.scm();
        for &o in &s.order.clone() {
            if s.units[o].owning_player == side {
                continue;
            }
            for w in &mut s.units[o].weapons {
                if w.acquired_target.is_some_and(|(t, _)| t == i) {
                    w.acquired_target = None;
                }
            }
        }
    }

    /// `ExtraMoveCheck`
    pub fn extra_move_check(&mut self, i: usize) -> bool {
        let u = self.su(i);
        if !["Fast Tank", "Fast Wheeled"].contains(&u.stat("movement_class").unwrap_or("")) || !u.has("recce") {
            return false;
        }
        self.roll() <= 20.0
    }

    /// `AttemptDigIn`
    pub fn attempt_dig_in(&mut self, i: usize) -> bool {
        let u = self.su(i);
        if !["Infantry", "Gun"].contains(&u.category()) || u.battle.dug_in || u.terrain.as_deref() == Some("Marsh") {
            return false;
        }
        let freezing = self.cd().weather.freezing;
        let chance = match u.terrain.as_deref() {
            Some("Wooden Buildings" | "Woods" | "Rubble") => 50.0,
            Some("Brush" | "Broken Ground") => {
                if freezing {
                    10.0
                } else {
                    30.0
                }
            }
            _ => {
                if freezing {
                    3.0
                } else {
                    15.0
                }
            }
        };
        if self.roll() <= chance {
            self.sum(i).battle.dug_in = true;
            return true;
        }
        false
    }

    /// `DoBogCheck`
    pub fn do_bog_check(&mut self, i: usize, forward: bool, pivot: bool, reposition: bool) {
        let u = self.su(i);
        if !["Vehicle", "Gun"].contains(&u.category()) {
            return;
        }
        let mut chance = u.battle.bog_chance;
        if pivot {
            chance *= 0.25;
        } else if reposition {
            chance *= 0.5;
        } else if !forward {
            chance *= 1.5;
        }
        if self.roll() <= round1(chance) {
            self.sum(i).bogged = true;
        }
    }

    /// `DoUnbogCheck`
    pub fn do_unbog_check(&mut self, i: usize) -> bool {
        if !self.su(i).bogged {
            return false;
        }
        self.sum(i).moving = true;
        if self.roll() > self.su(i).battle.bog_chance {
            self.sum(i).bogged = false;
            return true;
        }
        false
    }

    /// `BreakdownCheck`
    pub fn breakdown_check(&mut self, i: usize) -> bool {
        let u = self.su(i);
        if u.category() != "Vehicle" {
            return false;
        }
        let chance = if u.stats.contains_key("unreliable") { 3.0 } else { 0.8 };
        self.roll() <= chance
    }

    /// `VulnerableCrew`: positions with an exposed crewman able to act.
    pub fn vulnerable_crew(&self, i: usize) -> Vec<usize> {
        self.su(i)
            .positions
            .iter()
            .enumerate()
            .filter(|(_, p)| p.crewman.as_ref().is_some_and(|c| c.ce && c.alive && !["Unconscious", "Critical"].contains(&c.condition.as_str())))
            .map(|(n, _)| n)
            .collect()
    }

    /// `CalculateMoveChances`
    pub fn calculate_move_chances(&mut self, i: usize) {
        let u = self.su(i);
        let mut fwd = num(engine("BASE_FORWARD_MOVE_CHANCE"));
        let mut rev = num(engine("BASE_REVERSE_MOVE_CHANCE"));
        let mut bog = 1.0;
        let mc = u.stat("movement_class").unwrap_or("").to_string();
        match mc.as_str() {
            "Slow Tank" => {
                fwd -= 15.0;
                bog += 2.0;
            }
            "Fast Tank" => fwd += 10.0,
            "Half-Tracked" => {
                fwd += 5.0;
                bog -= 0.5;
            }
            "Wheeled" => {
                fwd += 5.0;
                bog += 4.0;
            }
            "Fast Wheeled" => {
                fwd += 15.0;
                bog += 5.0;
            }
            _ => {}
        }
        if u.has("powerful_engine") {
            fwd += 5.0;
            bog -= 0.5;
        }
        if u.has("HVSS") {
            fwd += 10.0;
            rev += 10.0;
            bog -= 1.5;
        }
        if let Some(effects) = u.terrain.as_deref().and_then(terrain_effects) {
            if !u.has("off_road") {
                if let Some(m) = effects.get("Movement Mod") {
                    fwd += num(m);
                    rev += num(m);
                }
                if let Some(m) = effects.get("Bog Mod") {
                    bog += num(m);
                }
            }
        }
        let ground = self.cd().weather.ground.clone();
        if ground != "Dry" {
            let wheeled = ["Fast Wheeled", "Wheeled"].contains(&mc.as_str());
            let half = mc == "Half-Tracked";
            let (mut m, mut b) = (0.0, 0.0);
            if ground == "Deep Snow" {
                (m, b) = if wheeled { (-45.0, 4.0) } else if half { (-20.0, 1.0) } else { (-30.0, 2.0) };
            } else if ground == "Muddy" || ground == "Snow" {
                (m, b) = if wheeled { (-25.0, 2.0) } else if half { (-10.0, 0.5) } else { (-15.0, 1.0) };
            }
            if (ground == "Snow" || ground == "Deep Snow") && self.camp().check_national_skill("Motti") {
                b = round1(b * 0.5);
            }
            fwd += m;
            rev += m;
            bog += b;
        }
        match u.stat("ground_pressure") {
            Some("Light") => bog = round1(bog * 0.8),
            Some("Heavy") => bog = round1(bog * 1.2),
            _ => {}
        }
        fwd += u.battle.forward_move_bonus;
        rev += u.battle.reverse_move_bonus;
        for pos in ["Commander", "Commander/Gunner"] {
            if let Some(c) = u.crewman_in(pos) {
                if c.current_cmd == "Direct Movement" {
                    fwd += 15.0;
                    rev += 10.0;
                }
                break;
            }
        }
        if let Some(d) = u.position_of("Driver") {
            if self.untrained_position(i, d) {
                fwd *= 0.4;
                rev *= 0.4;
            }
        }
        let b = &mut self.sum(i).battle;
        b.forward_move_chance = restrict(fwd);
        b.reverse_move_chance = restrict(rev);
        b.bog_chance = bog.clamp(0.5, 90.0);
    }

    /// `ResetForNewTurn`
    pub fn reset_for_new_turn(&mut self, i: usize, skip_smoke: bool) {
        if !skip_smoke && self.su(i).smoke > 0 {
            let mut roll = self.roll();
            if self.su(i).moving {
                roll -= 5.0;
            }
            match self.cd().weather.precipitation.as_str() {
                "Rain" => roll -= 5.0,
                "Heavy Rain" => roll -= 15.0,
                _ => {}
            }
            if roll <= 5.0 {
                self.sum(i).smoke -= 1;
                if i == self.pl() {
                    let t = if self.su(i).smoke > 0 {
                        "Some of the smoke nearby disperses somewhat."
                    } else {
                        "Some of the smoke nearby has dispersed."
                    };
                    self.bmsg(t, None);
                }
            }
        }
        let u = self.sum(i);
        u.moving = false;
        u.battle.previous_facing = u.facing;
        u.battle.previous_turret_facing = u.turret_facing;
        u.fired = false;
        for w in 0..self.su(i).weapons.len() {
            let wp = &mut self.sum(i).weapons[w];
            wp.fired = false;
            wp.maintained_rof = false;
            self.update_covered_hexes(i, w);
        }
        if i == self.pl() && self.sc().selected_weapon.is_none() && !self.su(i).weapons.is_empty() {
            self.scm().selected_weapon = Some(0);
        }
        for p in 0..self.su(i).positions.len() {
            self.update_visible_hexes(i, p);
            if self.su(i).positions[p].crewman.is_some() {
                self.do_injury_check(i, p);
            }
        }
        if i == self.pl() {
            for w in 0..self.su(i).weapons.len() {
                if self.attempt_unjam(i, w) {
                    let name = self.su(i).weapons[w].name().to_string();
                    self.bmsg(&format!("{name} is no longer jammed."), None);
                }
            }
        }
    }

    // ---------------------------------------------------------------------
    // spotting and line of sight

    /// `CalcSpotChance`; `crewman` the spotter's position if any.
    pub fn calc_spot_chance(&self, u1: usize, u2: usize, crewman: Option<usize>) -> f64 {
        let (a, b) = (self.su(u1), self.su(u2));
        let dist = distance(a.hx, a.hy, b.hx, b.hy);
        let base = engine("SPOT_BASE_CHANCE");
        let mut chance = num(&base[dist.clamp(0, 3) as usize]);
        match b.stat("size_class") {
            Some("Small") => chance -= 7.0,
            Some("Very Small") => chance -= 18.0,
            Some("Large") => chance += 7.0,
            Some("Very Large") => chance += 18.0,
            _ => {}
        }
        match self.cd().weather.precipitation.as_str() {
            "Rain" | "Snow" => chance -= 5.0 * dist as f64,
            "Heavy Rain" | "Blizzard" => chance -= 10.0 * dist as f64,
            _ => {}
        }
        match a.smoke + b.smoke {
            1 => chance *= 0.75,
            2 => chance *= 0.5,
            n if n > 2 => chance *= 0.3,
            _ => {}
        }
        if b.moving {
            chance *= 1.5;
        }
        if b.fired {
            chance *= 2.0;
        }
        if a.category() == "Infantry" && dist > 1 {
            chance *= if dist == 2 { 0.75 } else { 0.5 };
        }
        chance += self.tem(u2);
        if let Some(p) = crewman {
            if let Some(c) = &a.positions[p].crewman {
                chance += c.skill_mod(c.stat("Perception") as f64 * num(engine("PERCEPTION_SPOTTING_MOD")));
                if c.skills.iter().any(|s| s == "Eagle Eyed") && c.ce {
                    chance += c.skill_mod(10.0);
                }
                let head = c.injury.iter().find(|(k, _)| k == "Head & Neck").and_then(|(_, v)| v.clone());
                if head.is_some_and(|h| h != "Light") {
                    chance *= 0.5;
                }
            }
        }
        if !b.hull_down.is_empty() && b.hull_down.contains(&direction_toward(b.hx, b.hy, a.hx, a.hy)) {
            chance *= 0.5;
        }
        if b.battle.hit_by_fp {
            chance *= 4.0;
        }
        chance
    }

    /// `DoAISpotChecks`
    pub fn do_ai_spot_checks(&mut self, side: i32) {
        for u1 in self.sc().order.clone() {
            let s = self.sc();
            if !s.units[u1].alive || s.units[u1].owning_player != side {
                continue;
            }
            let list: Vec<usize> = s
                .order
                .iter()
                .copied()
                .filter(|&u2| {
                    let b = &s.units[u2];
                    b.owning_player != side && b.alive && !b.spotted && s.los(u1, u2)
                })
                .collect();
            for u2 in list {
                let chance = self.calc_spot_chance(u1, u2, None);
                if chance <= 0.0 {
                    continue;
                }
                if self.roll() <= chance {
                    self.sum(u2).spotted = true;
                    let b = self.su(u2);
                    let mut text = if u2 == self.pl() {
                        "You were".to_string()
                    } else {
                        let mut t = self.uname(u2);
                        if b.owning_player == 0 {
                            t += " was";
                        }
                        t
                    };
                    text += " spotted";
                    if b.owning_player == 0 {
                        text += &format!(" by {}", self.uname(u1));
                    }
                    text += "!";
                    let portrait = b.stat("portrait").map(str::to_string);
                    let h = (b.hx, b.hy);
                    self.battle_message(&text, false, portrait.as_deref(), Some(h));
                }
            }
        }
    }

    /// `DoSpotChecks`, for the player's unit and squad.
    pub fn do_spot_checks(&mut self, i: usize) {
        let u = self.su(i);
        if distance(0, 0, u.hx, u.hy) > 3 {
            return;
        }
        let mut positions: Vec<usize> = (0..u.positions.len()).collect();
        crate::cday::shuffle(&mut positions, &mut self.rng);
        for p in positions {
            let s = self.sc();
            let pos = &s.units[i].positions[p];
            if pos.crewman.is_none() {
                continue;
            }
            let side = s.units[i].owning_player;
            let list: Vec<usize> = s
                .order
                .iter()
                .copied()
                .filter(|&o| {
                    let b = &s.units[o];
                    b.owning_player != side && b.alive && !b.spotted && pos.visible_hexes.contains(&(b.hx, b.hy)) && s.los(i, o)
                })
                .collect();
            if list.is_empty() {
                continue;
            }
            let target = *self.rng.choice(&list);
            let chance = self.calc_spot_chance(i, target, Some(p));
            if self.roll() > chance {
                continue;
            }
            self.sum(target).spotted = true;
            let mut text = if i != self.pl() { "A squadmate reports: ".to_string() } else { String::new() };
            text += &format!("{} spotted!", self.uname(target));
            let t = self.su(target);
            let portrait = t.stat("portrait").map(str::to_string);
            let h = (t.hx, t.hy);
            self.battle_message(&text, false, portrait.as_deref(), Some(h));
        }
    }

    /// `DoLoSRoll`
    fn do_los_roll(&mut self, u1: usize, u2: usize) -> bool {
        let (a, b) = (self.su(u1), self.su(u2));
        if !a.alive || !b.alive {
            return false;
        }
        if a.battle.overrun && b.hx == 0 && b.hy == -1 {
            return true;
        }
        let mut chance = match distance(a.hx, a.hy, b.hx, b.hy) {
            0 => 100.0,
            1 => 95.0,
            2 => 85.0,
            3 => 70.0,
            _ => return false,
        };
        for t in [&a.terrain, &b.terrain].into_iter().flatten() {
            chance -= terrain_effects(t).and_then(|e| e.get("los_mod")).map(num).unwrap_or(0.0);
        }
        self.roll() <= chance
    }

    fn set_los(&mut self, u1: usize, u2: usize, v: bool) {
        self.sum(u1).battle.los.insert(u2, v);
        self.sum(u2).battle.los.insert(u1, v);
    }

    /// `GenerateLoS`
    pub fn generate_los(&mut self) {
        let order = self.sc().order.clone();
        for &u in &order {
            self.sum(u).battle.los.clear();
        }
        for &u1 in &order {
            for &u2 in &order {
                if self.su(u2).battle.los.contains_key(&u1) {
                    continue;
                }
                let v = u1 == u2 || self.same_side_same_hex(u1, u2) || self.do_los_roll(u1, u2);
                self.set_los(u1, u2, v);
            }
        }
    }

    fn same_side_same_hex(&self, u1: usize, u2: usize) -> bool {
        let (a, b) = (self.su(u1), self.su(u2));
        a.owning_player == b.owning_player && a.hx == b.hx && a.hy == b.hy
    }

    /// `GenerateUnitLoS`
    pub fn generate_unit_los(&mut self, u1: usize) {
        self.sum(u1).battle.los.clear();
        for u2 in self.sc().order.clone() {
            let v = u1 == u2 || self.same_side_same_hex(u1, u2) || self.do_los_roll(u1, u2);
            self.set_los(u1, u2, v);
        }
    }

    /// `DoAmbushRoll`
    pub fn do_ambush_roll(&mut self) {
        let s = self.sc();
        if !s.order.iter().any(|&u| s.units[u].owning_player == 1 && s.units[u].alive) {
            return;
        }
        if self.camp().check_national_skill("Motti") {
            return;
        }
        let mut chance = 10.0;
        if ["Heavy Rain", "Snow", "Blizzard"].contains(&self.cd().weather.precipitation.as_str()) {
            chance += 10.0;
        }
        if ["Advance", "Spearhead"].contains(&self.cd().mission.as_str()) {
            chance += 10.0;
        }
        match self.cd().hex(self.sc().zone).terrain_type.as_str() {
            "Forest" | "Villages" => chance += 10.0,
            "Hills" => chance += 15.0,
            _ => {}
        }
        let pu = self.su(self.pl());
        for pos in ["Commander", "Commander/Gunner"] {
            let Some(c) = pu.crewman_in(pos) else { continue };
            if c.skills.iter().any(|s| s == "Enemy Spotted!") {
                chance -= c.skill_mod(10.0);
                break;
            }
        }
        if pu.has("recce") {
            chance -= 5.0;
        }
        if pu.positions.iter().filter_map(|p| p.crewman.as_ref()).any(|c| !c.incapacitated() && c.ce) {
            chance -= 10.0;
        }
        if self.roll() <= chance.max(3.0) {
            self.scm().ambush = true;
        }
    }

    /// `CheckForEnd`
    pub fn check_for_end(&mut self) {
        let s = self.sc();
        let enemies = s.order.iter().any(|&u| {
            let x = &s.units[u];
            x.owning_player == 1 && x.alive && distance(0, 0, x.hx, x.hy) <= 3
        });
        if !enemies {
            self.bmsg("Victory! No enemy units remain in this area.", None);
            self.scm().finished = true;
            return;
        }
        let pu = self.su(self.pl());
        if !pu.positions.iter().filter_map(|p| p.crewman.as_ref()).any(|c| c.alive) {
            self.bmsg("Your crew is all dead.", None);
            let p = self.pl();
            self.destroy_me(p, None, None, false);
            return;
        }
        self.check_for_end_of_day();
    }

    // ---------------------------------------------------------------------
    // enemy forces

    /// `SpawnEnemyUnits`
    pub fn spawn_enemy_units(&mut self, reinforcement: bool) {
        if self.sc().enemy_nation.is_none() {
            let list = defs::strings(&self.camp().week()["enemy_nations"]);
            let n = self.rng.choice(&list).clone();
            self.scm().enemy_nation = Some(n);
        }
        let nation = self.sc().enemy_nation.clone().unwrap();
        let types = defs::strings(&self.camp().stats["enemy_unit_list"][&nation]);
        let unit_types = &defs::defs().unit_types;
        let mut num_units = 4;
        if reinforcement {
            num_units = 1;
        } else {
            let strength = self.cd().hex(self.sc().zone).enemy_strength;
            for _ in 0..3 {
                if self.rng.int(0, 10) >= strength {
                    num_units -= 1;
                }
            }
        }
        let class_odds: Vec<(String, f64)> = self
            .camp()
            .stats
            .get("enemy_unit_class_odds")
            .and_then(|v| v.as_object())
            .map(|m| m.iter().map(|(k, v)| (k.clone(), num(v))).collect())
            .unwrap_or_default();
        let today = self.camp().today.clone();
        let mut list: Vec<String> = Vec::new();
        let mut tries = 0;
        while (list.len() as i32) < num_units && tries < 10_000 {
            tries += 1;
            let class = loop {
                let (k, v) = self.rng.choice(&class_odds).clone();
                if self.roll() <= v {
                    break k;
                }
            };
            if let Some(id) = self.sc().class_type_dict.get(&class) {
                list.push(id.clone());
                continue;
            }
            let mut of_class: Vec<String> =
                types.iter().filter(|id| unit_types.get(*id).is_some_and(|u| text(&u["class"]) == class)).cloned().collect();
            if of_class.is_empty() {
                continue;
            }
            crate::cday::shuffle(&mut of_class, &mut self.rng);
            let mut selected = None;
            for id in of_class {
                let Some(rarity) = unit_types[&id].get("rarity").and_then(|r| r.as_object()) else {
                    selected = Some(id);
                    break;
                };
                let mut r: Option<i32> = None;
                for (date, chance) in rarity {
                    if r.is_none() {
                        if date.as_str() > today.as_str() {
                            break;
                        }
                        r = Some(int(chance));
                        continue;
                    }
                    if date.as_str() > today.as_str() {
                        break;
                    }
                    r = Some(int(chance));
                }
                let Some(r) = r else { continue };
                if self.roll() <= r as f64 {
                    selected = Some(id);
                    break;
                }
            }
            if let Some(id) = selected {
                list.push(id.clone());
                self.scm().class_type_dict.insert(class, id);
            }
        }

        let ambush = self.sc().ambush;
        for id in list {
            let cat = text(&unit_types[&id]["category"]).to_string();
            let mut dist = self.rng.int(1, 3);
            if dist == 1 && self.roll() <= 65.0 {
                dist += 1;
            }
            if cat == "Infantry" {
                if self.roll() <= 75.0 {
                    dist -= 1;
                }
            } else if cat == "Vehicle" && self.roll() <= 60.0 {
                dist += 1;
            }
            let dist = dist.clamp(1, 3);
            let mut hexes_list = ring(0, 0, dist);
            crate::cday::shuffle(&mut hexes_list, &mut self.rng);
            let mut spot = hexes_list[hexes_list.len() - 1];
            for &h in &hexes_list {
                spot = h;
                if !ambush && [5, 0, 1].contains(&direction_toward(h.0, h.1, 0, 0)) && self.roll() <= 85.0 {
                    continue;
                }
                break;
            }
            let Some(i) = self.new_enemy_unit(&id, &nation) else { continue };
            self.spawn_at(i, spot.0, spot.1);
            if cat == "Gun" {
                self.sum(i).battle.deployed = true;
            }
            if cat != "Infantry" {
                let dir = if ambush { direction_toward(spot.0, spot.1, 0, 0) } else { self.rng.int(0, 5) };
                let u = self.sum(i);
                u.facing = Some(dir);
                if u.stats.contains_key("turret") {
                    u.turret_facing = Some(dir);
                }
            }
            let mission = self.cd().mission.clone();
            if ["Infantry", "Gun"].contains(&cat.as_str()) && mission != "Fighting Withdrawal" {
                let (c1, c2, c3) = if ["Battle", "Counterattack"].contains(&mission.as_str()) { (5.0, 15.0, 25.0) } else { (2.0, 10.0, 15.0) };
                let roll = self.roll();
                let b = &mut self.sum(i).battle;
                if roll <= c1 {
                    b.fortified = true;
                } else if roll <= c2 {
                    b.entrenched = true;
                } else if roll <= c3 {
                    b.dug_in = true;
                }
            }
            let mut unit = i;
            if id == "Light Artillery Car" {
                // the original read a week's `enemy_nation`, which the
                // campaigns do not have, and crashed; the battle's nation.
                // As in the original, what follows applies to the locomotive
                if let Some(l) = self.new_enemy_unit("Locomotive", &nation) {
                    self.spawn_at(l, spot.0, spot.1);
                    self.sum(l).facing = Some(direction_toward(spot.0, spot.1, 0, 0));
                    unit = l;
                }
            }
            if let Some(t) = defs::get(&self.su(unit).stats, "transport").map(defs::strings) {
                if self.roll() <= 60.0 {
                    let pick = self.rng.choice(&t).clone();
                    self.sum(unit).battle.transport = Some(pick);
                } else {
                    let pick = self.rng.choice(&["Ammo", "Food", "Supplies"]).to_string();
                    self.sum(unit).battle.cargo = Some(pick);
                }
            }
            if reinforcement {
                self.generate_unit_los(unit);
            } else if self.cd().hex(self.sc().zone).controlled_by == 1 && self.cd().advancing_fire {
                self.pin_test(unit, 10.0);
            }
        }
    }

    /// An enemy unit with its crew and AI, not yet on the map.
    pub fn new_enemy_unit(&mut self, id: &str, nation: &str) -> Option<usize> {
        let mut u = Unit::new(id, &mut self.rng)?;
        u.owning_player = 1;
        u.nation = nation.to_string();
        u.battle.ai = Some(crate::ai::Ai::default());
        let today = self.camp().today.clone();
        u.generate_new_personnel(&CrewContext { player: false, national_skills: &[], today: &today }, &mut self.rng);
        Some(self.add_unit(u))
    }

    // ---------------------------------------------------------------------
    // attacks

    /// `CheckAttack`: "" if the attack can be made, else why not.
    pub fn check_attack(&self, att: usize, w: usize, tgt: usize, ignore_facing: bool) -> String {
        let s = self.sc();
        let a = &s.units[att];
        let wp = &a.weapons[w];
        let t = &s.units[tgt];
        let wtype = wp.stat("type").unwrap_or("");
        if att == s.player {
            let Some(fb) = defs::get(&wp.stats, "fired_by").map(defs::strings) else {
                return "No positions to fire this weapon".into();
            };
            let req = if wtype == "Gun" {
                "Operate Gun"
            } else if mg_weapons().iter().any(|m| m == wtype) {
                "Operate MG"
            } else {
                return "Unknown weapon type!".into();
            };
            if !fb.iter().any(|pos| a.crewman_in(pos).is_some_and(|c| c.current_cmd == req)) {
                return "No crewman operating this weapon".into();
            }
        }
        if wp.broken {
            return "Weapon is broken!".into();
        }
        if wp.jammed {
            return "Weapon is jammed!".into();
        }
        if wp.fired {
            return "Weapon has already fired this turn".into();
        }
        if wtype == "Close Combat" {
            if !t.spotted {
                return "Target location unknown".into();
            }
            if a.pinned {
                return "Cannot initiate Close Combat when Pinned".into();
            }
        } else if !s.los(att, tgt) {
            return "No Line of Sight to Target".into();
        }
        if !ignore_facing && !wp.covered_hexes.contains(&(t.hx, t.hy)) {
            return "Target not in weapon's covered arc".into();
        }
        if wtype == "Gun" {
            let Some(at) = &wp.ammo_type else { return "No ammo loaded".into() };
            let rr = wp.ready_rack.get(at).copied().unwrap_or(0);
            let st = wp.ammo_stores.as_ref().and_then(|s| s.get(at).copied()).unwrap_or(0);
            if !((wp.using_rr && rr > 0) || st > 0) {
                return "No more ammo of the selected type".into();
            }
            if at == "AP" && ["Infantry", "Gun"].contains(&t.category()) {
                return "AP has no effect on target".into();
            }
        }
        for (n, w2) in a.weapons.iter().enumerate() {
            if n == w || !w2.fired {
                continue;
            }
            if defs::get(&w2.stats, "firing_group") == defs::get(&wp.stats, "firing_group") {
                return "A weapon on this mount has already fired".into();
            }
        }
        if !a.hull_down.is_empty() && wp.stat("mount") == Some("Hull") && a.hull_down.contains(&direction_toward(a.hx, a.hy, t.hx, t.hy)) {
            return "Weapon blocked by HD status".into();
        }
        if !ignore_facing && wp.stat("mount") == Some("Turret") {
            if let Some(blocked) = defs::get(&wp.stats, "blocked_hull_dirs").map(defs::strings) {
                let d = constrain_dir(a.turret_facing.unwrap_or(0) - a.facing.unwrap_or(0));
                if blocked.contains(&d.to_string()) {
                    return "Weapon blocked by hull direction".into();
                }
            }
        }
        String::new()
    }

    /// `GetRelativeBearing`
    fn relative_bearing(&self, u1: usize, u2: usize) -> i32 {
        let (a, b) = (self.su(u1), self.su(u2));
        let (x1, y1) = plot_sq(a.hx, a.hy);
        let (x2, y2) = plot_sq(b.hx, b.hy);
        rectify(bearing(x1, y1, x2, y2) - a.facing.unwrap_or(0) * 60)
    }

    /// `GetFacing`: which of the target's faces the attacker sees.
    pub fn get_facing(&self, att: usize, tgt: usize, turret_facing: bool) -> &'static str {
        let mut b = self.relative_bearing(tgt, att);
        let t = self.su(tgt);
        if turret_facing {
            if let Some(tf) = t.turret_facing {
                b = rectify(b - (tf - t.facing.unwrap_or(0)) * 60);
            }
        }
        if b >= 300 || b <= 60 {
            "Front"
        } else if 140 < b && b < 220 {
            "Rear"
        } else {
            "Side"
        }
    }

    /// `CalcAttack`
    pub fn calc_attack(&self, att: usize, w: usize, tgt: usize, pivot: bool, turret_rotate: bool) -> Option<Profile> {
        let s = self.sc();
        let a = &s.units[att];
        let wp = &a.weapons[w];
        let t = &s.units[tgt];
        let wtype = wp.stat("type").unwrap_or("").to_string();
        let mg = mg_weapons();
        let is_mg = mg.contains(&wtype);
        let mut p = Profile {
            attacker: att,
            weapon: w,
            ammo_type: wp.ammo_type.clone(),
            target: tgt,
            kind: match wtype.as_str() {
                "Gun" => "Point Fire",
                "Small Arms" => "Area Fire",
                _ if is_mg => "Area Fire",
                "Close Combat" => "Close Combat",
                _ => return None,
            }
            .into(),
            ..Profile::default()
        };
        if let Some(fb) = defs::get(&wp.stats, "fired_by").map(defs::strings) {
            // `command_req` is only set for guns and MGs; for others the
            // original read an unset name (no such weapons have fired_by)
            let req = if wtype == "Gun" { "Operate Gun" } else { "Operate MG" };
            for (n, pos) in a.positions.iter().enumerate() {
                let Some(c) = &pos.crewman else { continue };
                if fb.contains(&pos.name) {
                    if c.current_cmd != req {
                        continue;
                    }
                    p.crewman = Some(n);
                    break;
                }
            }
        }
        let crew = p.crewman.and_then(|n| a.positions[n].crewman.as_ref());
        let dist = distance(a.hx, a.hy, t.hx, t.hy);
        let mut mods: Vec<(String, f64)> = Vec::new();
        p.critical_hit = num(engine("CRITICAL_HIT"));
        let pf = engine("PF_BASE_CHANCE");
        let pf_at = |d: i32, k: usize| num(&pf[d.clamp(0, 3) as usize][k]);
        let precip = self.cd().weather.precipitation.clone();
        let size_mod = |mods: &mut Vec<(String, f64)>| {
            if let Some(sc) = t.stat("size_class") {
                if sc != "Normal" {
                    mods.push((format!("{sc} Target"), engine("PF_SIZE_MOD").get(sc).map(num).unwrap_or(0.0)));
                }
            }
        };
        let squad_pivot = s.player_pivot != 0 && (att == s.player || s.squad.contains(&att));

        if p.kind == "Point Fire" {
            if let Some(c) = crew {
                if c.skills.iter().any(|x| x == "Knows Weak Spots") && wtype == "Gun" && t.has("armour") {
                    p.critical_hit += c.skill_mod(2.0);
                }
            }
            p.base_chance = if !t.spotted {
                pf_at(dist, 1)
            } else if t.category() == "Vehicle" {
                pf_at(dist, 0)
            } else {
                pf_at(dist, 1)
            };
            if a.moving {
                mods.push(("Attacker Moving".into(), -60.0));
            } else if pivot || a.facing != a.battle.previous_facing {
                let m = if wp.stats.contains_key("turntable") { -15.0 } else { -35.0 };
                mods.push(("Attacker Pivoted".into(), m));
            } else if squad_pivot {
                mods.push(("Attacker Pivoted".into(), -35.0));
            } else if wp.stat("mount") == Some("Turret") && (turret_rotate || a.turret_facing != a.battle.previous_turret_facing) {
                let m = if a.stat("turret") == Some("Fast") { -10.0 } else { -20.0 };
                mods.push(("Turret Rotated".into(), m));
            }
            if a.pinned {
                mods.push(("Attacker Pinned".into(), -60.0));
            } else if a.reduced {
                mods.push(("Attacker Reduced".into(), -40.0));
            }
            match precip.as_str() {
                "Rain" => mods.push(("Rain".into(), -5.0 * dist as f64)),
                "Snow" => mods.push(("Snow".into(), -10.0 * dist as f64)),
                "Heavy Rain" => mods.push(("Heavy Rain".into(), -15.0 * dist as f64)),
                "Blizzard" => mods.push(("Blizzard".into(), -20.0 * dist as f64)),
                _ => {}
            }
            let smoke = a.smoke + t.smoke;
            if smoke >= 2 {
                mods.push(("Smoke".into(), -50.0));
            } else if smoke == 1 {
                mods.push(("Smoke".into(), -25.0));
            }
            if !t.spotted {
                mods.push(("Unspotted Target".into(), -20.0));
            } else {
                if let Some((ac, level)) = wp.acquired_target {
                    if ac == tgt {
                        let bonus = &engine("AC_BONUS")[dist.clamp(0, 3) as usize][level as usize];
                        mods.push((if level == 1 { "Acquired Target+" } else { "Acquired Target" }.into(), num(bonus)));
                    }
                }
                if t.moving {
                    let m = match a.category() {
                        "Vehicle" => {
                            if wp.stat("mount") == Some("Hull") {
                                -45.0
                            } else {
                                -30.0
                            }
                        }
                        "Gun" => {
                            if a.stats.contains_key("turntable") {
                                -15.0
                            } else {
                                -30.0
                            }
                        }
                        _ => 0.0,
                    };
                    if m != 0.0 {
                        mods.push(("Target Moving".into(), m));
                    }
                }
                size_mod(&mut mods);
                if t.battle.overrun {
                    if a.hx == 0 && a.hy == -1 {
                        mods.push(("Point Blank Range".into(), 20.0));
                    }
                } else {
                    let tem = self.tem(tgt);
                    if tem != 0.0 {
                        mods.push((t.terrain.clone().unwrap_or_default(), tem));
                    }
                }
            }
            match wp.stat("long_range") {
                Some("S") if dist > 1 => mods.push(("Short Gun".into(), -12.0)),
                Some("L") if dist > 1 => mods.push(("Long Gun".into(), 12.0)),
                Some("LL") if dist == 1 => mods.push(("Long Gun".into(), 12.0)),
                Some("LL") if dist >= 2 => mods.push(("Long Gun".into(), 24.0)),
                _ => {}
            }
            if wtype == "Gun" {
                // the original passed two arguments to `append` here and
                // crashed (only one gun has APCR)
                if let Some(at) = &p.ammo_type {
                    if (at == "APCR" || at == "APDS") && dist == 3 {
                        mods.push((at.clone(), -12.0));
                    }
                }
                let calibre = if wp.name() == "AT Rifle" { 20 } else { wp.stats.get("calibre").map(int).unwrap_or(0) };
                let mut cm = 0;
                if calibre <= 40 && dist >= 2 {
                    cm -= 1;
                }
                if calibre <= 57 {
                    if dist == 2 {
                        cm -= 1;
                    } else if dist == 3 {
                        cm -= 2;
                    }
                }
                if cm < 0 {
                    mods.push(("Small Calibre".into(), 8.0 * cm as f64));
                }
            }
        } else if p.kind == "Area Fire" {
            let overrun_attack = a.battle.overrun && t.spotted && ["Infantry", "Gun"].contains(&t.category()) && t.hx == 0 && t.hy == -1;
            let mut fp = defs::get(&wp.stats, "fp").map(int).unwrap_or(0);
            if dist == 0 {
                fp *= 2;
            }
            p.base_fp = fp;
            let mut base = if t.category() == "Vehicle" { num(engine("VEH_FP_BASE_CHANCE")) } else { num(engine("INF_FP_BASE_CHANCE")) };
            let (step, stepm) = (num(engine("FP_CHANCE_STEP")), num(engine("FP_CHANCE_STEP_MOD")));
            for n in 2..=fp {
                base += step * stepm.powi(n - 1);
            }
            p.base_chance = round1(base);
            let base = p.base_chance;
            if overrun_attack {
                mods.push(("Overrun Attack".into(), base));
            } else if a.moving {
                mods.push(("Attacker Moving".into(), -round1(base / 2.0)));
            } else if a.facing != a.battle.previous_facing {
                mods.push(("Attacker Pivoted".into(), -round1(base / 3.0)));
            } else if att == s.player && s.player_pivot != 0 {
                mods.push(("Attacker Pivoted".into(), -round1(base / 3.0)));
            } else if wp.stat("mount") == Some("Turret") && a.turret_facing != a.battle.previous_turret_facing {
                mods.push(("Turret Rotated".into(), -round1(base / 4.0)));
            }
            if a.pinned {
                mods.push(("Attacker Pinned".into(), -round1(base / 2.0)));
            } else if a.reduced {
                mods.push(("Attacker Reduced".into(), -round1(base / 3.0)));
            }
            let smoke = a.smoke + t.smoke;
            if smoke > 0 {
                let m = if smoke >= 2 { round1(base / 2.0) } else { round1(base / 3.0) };
                mods.push(("Smoke".into(), -m));
            }
            if !t.spotted {
                mods.push(("Unspotted Target".into(), -20.0));
            } else {
                if let Some((ac, level)) = wp.acquired_target {
                    if ac == tgt {
                        let mut m = round1(15.0 + level as f64 * 15.0);
                        if dist == 3 {
                            m = round1(m * 1.15);
                        }
                        mods.push((if level == 1 { "Acquired Target+" } else { "Acquired Target" }.into(), m));
                    }
                }
                if t.moving && t.category() == "Infantry" {
                    mods.push(("Infantry Moving".into(), round1(base / 2.0)));
                } else if t.stat("class") == Some("Team") {
                    mods.push(("Small Team".into(), -20.0));
                }
                size_mod(&mut mods);
                if !overrun_attack && t.has("gun_shield") && self.get_facing(att, tgt, false) == "Front" {
                    mods.push(("Gun Shield".into(), -15.0));
                }
            }
            if t.battle.fortified {
                mods.push(("Target Fortified".into(), -50.0));
            } else if t.battle.entrenched {
                mods.push(("Target Entrenched".into(), if overrun_attack { -20.0 } else { -30.0 }));
            } else if t.battle.dug_in {
                mods.push(("Target Dug-in".into(), if overrun_attack { -5.0 } else { -15.0 }));
            }
        } else {
            p.base_chance = if t.category() == "Vehicle" { pf_at(0, 0) } else { pf_at(0, 1) };
            if a.reduced {
                mods.push(("Attacker Reduced".into(), -40.0));
            }
            if t.smoke >= 2 {
                mods.push(("Smoke".into(), -25.0));
            } else if t.smoke == 1 {
                mods.push(("Smoke".into(), -10.0));
            }
            if t.moving && t.category() == "Vehicle" {
                mods.push(("Moving Target".into(), -30.0));
            }
            size_mod(&mut mods);
            let tem = self.tem(tgt);
            if tem != 0.0 {
                mods.push((t.terrain.clone().unwrap_or_default(), tem));
            }
            if t.battle.fortified {
                mods.push(("Target Fortified".into(), -20.0));
            }
        }

        // the commander directing fire
        if let Some(c) = a.crewman_in("Commander") {
            if c.current_cmd == "Direct Fire" {
                let m = c.skill_mod(if c.ce { 7.0 } else { 3.5 });
                if m > 0.0 {
                    mods.push(("Cmdr Direction".into(), m));
                    if c.skills.iter().any(|x| x == "Fire Spotter") {
                        mods.push(("Fire Spotter".into(), c.skill_mod(3.0)));
                    }
                    if c.skills.iter().any(|x| x == "MG Spotter") && is_mg {
                        mods.push(("MG Spotter".into(), c.skill_mod(7.0)));
                    }
                    if c.skills.iter().any(|x| x == "Gun Spotter") && wtype == "Gun" {
                        mods.push(("Gun Spotter".into(), c.skill_mod(7.0)));
                    }
                }
            }
        }

        if let (Some(n), Some(c)) = (p.crewman, crew) {
            if self.untrained_position(att, n) {
                mods.push(("Untrained Position".into(), -50.0));
            } else if wtype == "Gun" {
                let has = |x: &str| c.skills.iter().any(|s| s == x);
                if has("Crack Shot") {
                    mods.push(("Crack Shot".into(), c.skill_mod(3.0)));
                }
                if t.moving && has("Target Tracker") {
                    mods.push(("Target Tracker".into(), c.skill_mod(7.0)));
                }
                if dist == 3 && has("Sniper") {
                    mods.push(("Sniper".into(), c.skill_mod(7.0)));
                }
                if ["Rain", "Snow", "Heavy Rain", "Blizzard"].contains(&precip.as_str()) && has("Target Focus") {
                    let mut m = mods.last().map(|x| x.1).unwrap_or(0.0);
                    for (t, v) in &mods {
                        if ["Rain", "Snow", "Heavy Rain", "Blizzard"].contains(&t.as_str()) {
                            m = *v;
                            break;
                        }
                    }
                    let sm = c.skill_mod(8.0).min(m.abs());
                    mods.push(("Target Focus".into(), sm));
                }
            }
            for (k, v) in &c.injury {
                if !k.contains("Arm") {
                    continue;
                }
                if v.as_deref().is_some_and(|v| ["Heavy", "Serious", "Critical"].contains(&v)) {
                    mods.push(("Arm/Hand Injury".into(), -15.0));
                }
            }
        }

        let total: f64 = mods.iter().map(|m| m.1).sum();
        p.modifier_list = mods;
        p.final_chance = restrict(p.base_chance + total);
        if p.kind == "Area Fire" {
            p.full_effect = restrict((p.base_chance + total) * num(engine("FP_FULL_EFFECT")));
            p.critical_effect = restrict((p.base_chance + total) * num(engine("FP_CRIT_EFFECT")));
        }
        Some(p)
    }

    /// `CalcAP`: the armour penetration profile for a hit.
    pub fn calc_ap(&self, mut p: Profile, air_attack: bool) -> Profile {
        p.kind = "ap".into();
        let s = self.sc();
        let a = &s.units[p.attacker];
        let wp = &a.weapons[p.weapon];
        let t = &s.units[p.target];
        let mut location = p.location.clone().unwrap_or_else(|| "Hull".into());
        let turret_facing = !(location == "Hull" || t.turret_facing.is_none());
        let facing = if air_attack || wp.stat("type") == Some("Close Combat") { "Side" } else { self.get_facing(p.attacker, p.target, turret_facing) };
        let rear = facing == "Rear";
        let facing = if rear { "Side" } else { facing };
        let hit_location = format!("{location}_{facing}").to_lowercase();
        if location == "Turret" && t.turret_facing.is_none() {
            location = "Upper Hull".into();
        }
        p.location_desc = format!("{location} {}", if rear { "Rear" } else { facing });

        let armour = defs::get(&t.stats, "armour").and_then(|v| v.as_object());
        let mut unarmoured = true;
        if let Some(arm) = armour {
            if !(t.stats.contains_key("open_rear_turret") && hit_location == "turret_rear") {
                if arm.get(&hit_location).map(text) != Some("-") {
                    unarmoured = false;
                }
            }
        }
        let wtype = wp.stat("type").unwrap_or("");
        let calibre_s = wp.stat("calibre").map(str::to_string);
        let ammo = p.ammo_type.clone().unwrap_or_default();
        let base_score: i32 = if mg_weapons().iter().any(|m| m == wtype) {
            if calibre_s.as_deref() == Some("15") { 5 } else { 4 }
        } else if wp.name() == "AT Rifle" {
            if ["Soviet Union", "Finland", "Japan"].contains(&a.nation.as_str()) { 6 } else { 5 }
        } else if wtype == "Close Combat" {
            let b = engine("CC_WEAPON_AP").get(wp.name()).map(int).unwrap_or(2);
            if unarmoured { (b as f64 * 1.5) as i32 } else { b }
        } else {
            let calibre_n = calibre_s.as_deref().map(|c| c.parse::<i32>().unwrap_or(0)).unwrap_or(0);
            let unarmoured_ap = |c: i32| match c {
                ..=28 => 7,
                29..=57 => 8,
                58..=77 => 9,
                78..=95 => 10,
                _ => 11,
            };
            let mut cal = calibre_s.clone().unwrap_or_default();
            if let Some(lr) = wp.stat("long_range") {
                cal += lr;
            }
            if ammo == "AP" {
                if unarmoured {
                    unarmoured_ap(calibre_n)
                } else {
                    match cal.as_str() {
                        "15" => 5,
                        "20L" => 6,
                        "20LL" | "25LL" | "37S" => 7,
                        "37" | "47S" | "57S" | "70S" => 8,
                        "37L" | "57" | "65S" | "76S" => 9,
                        "40L" | "45L" | "47" | "75S" => 10,
                        "37LL" | "45LL" | "47L" | "50" => 11,
                        "76" | "84S" => 12,
                        "50L" | "120S" => 13,
                        "75" | "105" => 14,
                        "57L" | "57LL" => 15,
                        "75L" | "76L" | "85L" | "150S" | "152S" => 17,
                        "77L" | "200L" => 19,
                        "88L" => 20,
                        "90L" | "105L" | "150" | "152" | "155" => 21,
                        "75LL" | "76LL" => 23,
                        "122L" => 25,
                        "88LL" | "100L" | "120L" => 27,
                        _ => {
                            log::error!("not able to find AP score for {cal}");
                            2
                        }
                    }
                }
            } else if ammo == "APCR" || ammo == "APDS" {
                if unarmoured {
                    unarmoured_ap(calibre_n)
                } else if ammo == "APDS" {
                    match cal.as_str() {
                        "57L" => 18,
                        "76LL" => 25,
                        _ => 2,
                    }
                } else {
                    match cal.as_str() {
                        "37L" => 10,
                        "28LL" | "45L" => 12,
                        "45LL" | "47L" => 13,
                        "40LL" | "50" => 14,
                        "76L" => match a.nation.as_str() {
                            "Soviet Union" => 14,
                            "United States of America" => 22,
                            _ => 20,
                        },
                        "50L" => 17,
                        "57LL" => 18,
                        "85L" => 19,
                        "75L" => 20,
                        "88L" => 23,
                        "90L" => 27,
                        _ => 2,
                    }
                }
            } else {
                // HE (and, as the original has no case for it, anything
                // else: smoke never gets here)
                if unarmoured {
                    match calibre_n {
                        ..=20 => 6,
                        21..=30 => 8,
                        31..=40 => 9,
                        41..=50 => 10,
                        51..=70 => 12,
                        71..=80 => 14,
                        81..=100 => 16,
                        101..=120 => 18,
                        _ => 20,
                    }
                } else {
                    match calibre_n {
                        ..=20 => 3,
                        21..=30 => 4,
                        31..=40 => 5,
                        41..=50 => 6,
                        51..=70 => 7,
                        71..=80 => 8,
                        81..=100 => 10,
                        101..=120 => 12,
                        _ => 16,
                    }
                }
            }
        };
        p.base_chance = base_score as f64;
        let mut mods: Vec<(String, f64)> = Vec::new();
        if rear {
            mods.push(("Rear Facing".into(), 1.0));
        }
        if p.result == "CRITICAL HIT" {
            mods.push(("Critical Hit".into(), base_score as f64));
        }
        let dist = distance(a.hx, a.hy, t.hx, t.hy);
        let cal_i = calibre_s.as_deref().map(|c| c.parse::<i32>().unwrap_or(0));
        if wp.name() == "AT Rifle" {
            mods.push(if dist <= 1 { ("Close Range".into(), 2.0) } else { ("Long Range".into(), -2.0) });
        } else if ammo == "AP" && cal_i.is_some() && !unarmoured {
            let c = cal_i.unwrap();
            let m = if c <= 25 {
                match dist {
                    ..=1 => Some(("Close Range", 2.0)),
                    2 => Some(("Medium Range", -1.0)),
                    _ => Some(("Long Range", -2.0)),
                }
            } else if c <= 57 {
                match dist {
                    ..=1 => Some(("Close Range", 1.0)),
                    2 => Some(("Medium Range", -1.0)),
                    _ => Some(("Long Range", -2.0)),
                }
            } else {
                match dist {
                    ..=1 => Some(("Close Range", 1.0)),
                    2..=3 => Some(("Long Range", -1.0)),
                    _ => None,
                }
            };
            if let Some((t, v)) = m {
                mods.push((t.into(), v));
            }
        } else if ammo == "APCR" && cal_i.is_some() && !unarmoured {
            let c = cal_i.unwrap();
            let (cl, med, long) = if c <= 57 { (2.0, -2.0, -4.0) } else { (2.0, -1.0, -3.0) };
            mods.push(match dist {
                ..=1 => ("Close Range".into(), cl),
                2 => ("Medium Range".into(), med),
                _ => ("Long Range".into(), long),
            });
        } else if ammo == "APDS" && !unarmoured && dist <= 1 {
            mods.push(("Close Range".into(), 1.0));
        }
        if !unarmoured {
            let ta = armour.and_then(|a| a.get(&hit_location)).map(int).unwrap_or(0);
            if ta >= 0 {
                mods.push(("Armour".into(), -ta as f64));
            }
        }
        let total: f64 = mods.iter().map(|m| m.1).sum();
        p.modifier_list = mods;
        let final_score = base_score + total as i32;
        p.final_score = final_score;
        p.final_chance = if final_score < 2 {
            0.0
        } else if final_score >= 12 {
            100.0
        } else {
            odds_2d6(final_score)
        };
        p
    }

    /// `DoAttackRoll`: roll, and set the profile's result. The attack
    /// window shows the roll when the player is involved.
    pub fn do_attack_roll(&mut self, mut p: Profile) -> Profile {
        let pl = self.pl();
        let involved = p.attacker == pl || p.target == pl;
        let mut result = String::new();
        if p.kind == "ap" {
            if p.final_chance == 0.0 {
                result = "NO PENETRATION".into();
            } else if p.final_chance == 100.0 {
                result = "PENETRATED".into();
            }
        }
        if result.is_empty() {
            let mut roll = self.roll();
            if involved {
                for n in 0..6 {
                    roll = self.roll();
                    if n == 5 && p.target == pl && self.cd().fate_points > 0 && (roll <= p.final_chance || roll <= p.critical_hit) {
                        let large = defs::get(&self.su(p.attacker).weapons[p.weapon].stats, "calibre").is_some_and(|c| int(c) > 25);
                        if (p.kind == "Point Fire" && large) || p.kind == "ap" {
                            self.cdm().fate_points -= 1;
                            roll = (p.final_chance + self.rng.int(10, 500) as f64 / 10.0).min(100.0);
                        }
                    }
                    self.attack_roll_shown(Some(roll));
                    self.wait(15, false);
                }
            }
            p.roll = roll;
            p.location = Some(if self.roll() <= 75.0 { "Hull" } else { "Turret" }.into());
            let (cmiss, chit) = (num(engine("CRITICAL_MISS")), num(engine("CRITICAL_HIT")));
            if p.kind == "ap" {
                result = if roll >= cmiss {
                    "NO PENETRATION"
                } else if roll <= chit || roll <= p.final_chance {
                    "PENETRATED"
                } else {
                    "NO PENETRATION"
                }
                .into();
            } else if p.kind == "Area Fire" {
                if roll <= p.critical_effect {
                    result = "CRITICAL EFFECT".into();
                    p.effective_fp = p.base_fp * 2;
                } else if roll <= p.full_effect {
                    result = "FULL EFFECT".into();
                    p.effective_fp = p.base_fp;
                } else if roll <= p.final_chance {
                    result = "PARTIAL EFFECT".into();
                    p.effective_fp = p.base_fp / 2;
                } else {
                    result = "NO EFFECT".into();
                }
                if result == "FULL EFFECT" || result == "CRITICAL EFFECT" {
                    let s = self.sc();
                    let (a, t) = (&s.units[p.attacker], &s.units[p.target]);
                    let wt = a.weapons[p.weapon].stat("type").unwrap_or("");
                    if mg_weapons().iter().any(|m| m == wt) && t.has("armour") && distance(a.hx, a.hy, t.hx, t.hy) <= int(engine("MG_AP_RANGE")) {
                        result = if result == "FULL EFFECT" { "HIT" } else { "CRITICAL HIT" }.into();
                    }
                }
            } else {
                result = if roll >= cmiss {
                    "MISS"
                } else if roll <= p.critical_hit {
                    "CRITICAL HIT"
                } else if roll <= p.final_chance {
                    "HIT"
                } else {
                    "MISS"
                }
                .into();
            }
            if (result == "HIT" || result == "CRITICAL HIT") && p.location.as_deref() == Some("Hull") {
                let s = self.sc();
                let (a, t) = (&s.units[p.attacker], &s.units[p.target]);
                if !t.hull_down.is_empty() && t.hull_down.contains(&direction_toward(t.hx, t.hy, a.hx, a.hy)) {
                    result = "MISS - HULL DOWN".into();
                }
            }
        }
        if p.result.is_empty() && result == "MISS - HULL DOWN" && involved {
            crate::sound::play("hull_down_save");
        }
        p.result = result;
        p
    }

    // ---------------------------------------------------------------------
    // attacking

    /// `Unit.Attack`: fire a weapon at a target, again while the rate of
    /// fire is kept and the player wants to. False if it could not be made.
    pub fn attack(&mut self, att: usize, w: usize, tgt: Option<usize>) -> bool {
        let Some(tgt) = tgt else { return false };
        if !self.check_attack(att, w, tgt, false).is_empty() {
            return false;
        }
        let pl = self.pl();
        // the original asks for `GetStat('Mount')`, a stat no weapon has,
        // so a rotated turret never loses its other weapons' acquired
        // targets (kept)
        if tgt == pl {
            let text = format!("{} attacks you with {}", self.uname(att), self.su(att).weapons[w].name());
            let h = (self.su(att).hx, self.su(att).hy);
            self.bmsg(&text, Some(h));
        }
        let display = att == pl || (self.su(att).spotted && tgt == pl);
        loop {
            if att == pl && self.jam_test(att, w) {
                let name = self.su(att).weapons[w].name().to_string();
                self.bmsg(&format!("{name} has jammed!"), None);
                break;
            }
            let Some(mut p) = self.calc_attack(att, w, tgt, false, false) else { break };
            if display {
                self.show_attack(&p, true);
                if !self.su(att).weapons[w].maintained_rof && self.wait_for_continue(tgt != pl) {
                    self.close_attack();
                    return true;
                }
            }
            {
                let u = self.sum(att);
                u.weapons[w].fired = true;
                u.fired = true;
                if p.kind == "Close Combat" {
                    u.moving = true;
                    u.battle.dug_in = false;
                    u.battle.entrenched = false;
                    u.battle.fortified = false;
                }
                let wp = &mut u.weapons[w];
                if wp.stat("type") == Some("Gun") {
                    if let Some(t) = wp.ammo_type.clone() {
                        // as in the original, a shot kept by rate of fire is
                        // not checked again, so the stores can go below zero
                        if wp.using_rr && wp.ready_rack.get(&t).copied().unwrap_or(0) > 0 {
                            *wp.ready_rack.get_mut(&t).unwrap() -= 1;
                        } else {
                            if wp.using_rr {
                                wp.using_rr = false;
                            }
                            if let Some(n) = wp.ammo_stores.as_mut().and_then(|s| s.get_mut(&t)) {
                                *n -= 1;
                            }
                        }
                    }
                }
            }
            self.animate_attack(att, w, tgt);
            p = self.do_attack_roll(p);
            if display {
                self.show_attack(&p, false);
            }
            // rate of fire, for the player (`DoAttackRoll`)
            if p.kind != "ap" && defs::get(&self.su(att).weapons[w].stats, "rof").is_some() && att == pl {
                let kept = self.roll() <= self.rof_chance(att, w);
                self.sum(att).weapons[w].maintained_rof = kept;
                self.show_attack_rof(kept);
            }
            let wt = self.su(att).weapons[w].stat("type").unwrap_or("").to_string();
            if wt == "Gun" || mg_weapons().contains(&wt) {
                self.add_acquired_target(att, w, tgt);
            }
            let mut finished = true;
            if att == pl && self.weapon_breakdown_test(att, w) {
                let name = self.su(att).weapons[w].name().to_string();
                self.battle_message(&format!("{name} has broken down! It may not be used again for the rest of the day."), true, None, None);
                self.sum(att).weapons[w].maintained_rof = false;
            }
            if display {
                if let Some(again) = self.wait_attack_result(att == pl && self.su(att).weapons[w].maintained_rof) {
                    finished = !again;
                }
            }
            self.apply_attack_result(&p);
            if !display && tgt == pl {
                let h = (self.su(att).hx, self.su(att).hy);
                self.bmsg(&format!("Result: {}", p.result), Some(h));
            }
            if finished {
                break;
            }
        }
        self.close_attack();
        self.sum(att).weapons[w].maintained_rof = false;
        true
    }

    /// The results of one attack, applied to its target.
    fn apply_attack_result(&mut self, p: &Profile) {
        let tgt = p.target;
        let pl = self.pl();
        let t_cat = self.su(tgt).category().to_string();
        let t_armour = self.su(tgt).has("armour");
        let hit = p.result == "HIT" || p.result == "CRITICAL HIT";
        let add_fp = |g: &mut Game, fp: i32| {
            let t = g.sum(tgt);
            t.battle.fp_to_resolve += fp;
            if !t.spotted {
                t.battle.hit_by_fp = true;
            }
        };
        match p.kind.as_str() {
            "Area Fire" => {
                if ["CRITICAL EFFECT", "FULL EFFECT", "PARTIAL EFFECT"].contains(&p.result.as_str()) {
                    add_fp(self, p.effective_fp);
                } else if hit {
                    self.sum(tgt).battle.ap_hits_to_resolve.push(p.clone());
                    self.sum(tgt).battle.fp_to_resolve += p.effective_fp;
                }
            }
            "Close Combat" if hit => {
                if ["Infantry", "Gun"].contains(&t_cat.as_str()) || !t_armour {
                    let mut fp = defs::get(&self.su(p.attacker).weapons[p.weapon].stats, "fp").map(int).unwrap_or(0);
                    if p.result == "CRITICAL HIT" {
                        fp *= 2;
                    }
                    add_fp(self, fp);
                } else {
                    self.sum(tgt).battle.ap_hits_to_resolve.push(p.clone());
                }
            }
            "Point Fire" if hit => {
                if p.attacker == pl {
                    self.add_record("Gun Hits", 1);
                }
                let ammo = p.ammo_type.clone().unwrap_or_default();
                let effective = |g: &Game| {
                    let mut fp = g.su(p.attacker).weapons[p.weapon].effective_fp();
                    if p.result == "CRITICAL HIT" {
                        fp *= 2;
                    }
                    fp
                };
                if ammo == "Smoke" {
                    self.sum(tgt).smoke = 2;
                } else if ["Infantry", "Gun"].contains(&t_cat.as_str()) {
                    if ammo == "HE" {
                        let mut fp = effective(self);
                        match self.cd().weather.ground.as_str() {
                            "Deep Snow" => fp = (fp as f64 * 0.5) as i32,
                            "Muddy" | "Snow" => fp = (fp as f64 * 0.75) as i32,
                            _ => {}
                        }
                        let b = self.su(tgt).battle.clone();
                        if b.fortified {
                            if self.roll() <= fp as f64 {
                                {
                                    let t = self.sum(tgt);
                                    t.battle.fortified = false;
                                    t.terrain = Some("Rubble".into());
                                }
                                let t = self.su(tgt);
                                if t.owning_player == 0 || t.spotted {
                                    let text = format!("{}'s fortification has been destroyed.", t.unit_id);
                                    let h = (t.hx, t.hy);
                                    self.bmsg(&text, Some(h));
                                }
                                fp *= 4;
                            } else {
                                fp = (fp as f64 * 0.1) as i32;
                            }
                        } else if b.entrenched {
                            fp = (fp as f64 * 0.25) as i32;
                        } else if b.dug_in {
                            fp = (fp as f64 * 0.5) as i32;
                        }
                        add_fp(self, fp);
                    } else if ammo == "AP" {
                        self.sum(tgt).battle.fp_to_resolve += 1;
                    }
                } else if !t_armour {
                    if ammo == "AP" {
                        self.sum(tgt).battle.ap_hits_to_resolve.push(p.clone());
                    } else {
                        let fp = effective(self);
                        add_fp(self, fp);
                    }
                } else {
                    self.sum(tgt).battle.ap_hits_to_resolve.push(p.clone());
                }
            }
            _ => {}
        }
    }

    /// `ResolveAPHits`
    pub fn resolve_ap_hits(&mut self, i: usize) {
        if !self.su(i).alive || self.su(i).battle.ap_hits_to_resolve.is_empty() {
            return;
        }
        if ["Infantry", "Gun"].contains(&self.su(i).category()) {
            self.sum(i).battle.ap_hits_to_resolve.clear();
            return;
        }
        self.move_to_top_of_stack(i);
        let pl = self.pl();
        let hits = std::mem::take(&mut self.sum(i).battle.ap_hits_to_resolve);
        for p in hits {
            let mut p = self.calc_ap(p, false);
            let involved = p.attacker == pl || i == pl;
            if involved {
                self.show_attack(&p, true);
                if p.final_chance != 0.0 && p.final_chance != 100.0 {
                    self.wait_for_continue(false);
                }
            }
            p = self.do_attack_roll(p);
            if involved {
                self.show_attack(&p, false);
                self.wait_for_continue(false);
            }
            self.close_attack();
            if p.result == "NO PENETRATION" {
                crate::sound::play_armour_save(&mut self.rng);
            }
            if p.result == "NO PENETRATION" {
                let had = self.su(i).weapons.iter().any(|w| w.acquired_target.is_some());
                if had && self.roll() > p.final_chance * 1.75 {
                    for w in &mut self.sum(i).weapons {
                        w.acquired_target = None;
                    }
                    if i == pl {
                        self.bmsg("The impact has knocked your weapons off target. All acquired targets lost.", None);
                    }
                }
                if i == pl {
                    let diff = p.final_chance - p.roll;
                    let margin = num(engine("AP_STUN_MARGIN"));
                    if diff > 0.0 && diff <= margin {
                        for n in 0..self.su(i).positions.len() {
                            if self.su(i).positions[n].crewman.is_none() {
                                continue;
                            }
                            if !self.stun_check(i, n, margin - diff) {
                                let name = self.su(i).positions[n].name.clone();
                                self.bmsg(&format!("Your {name} is Stunned from the impact."), None);
                            }
                        }
                    }
                }
            } else if p.result == "PENETRATED" {
                if i == pl {
                    let diff = p.roll - p.final_chance;
                    let roll = self.roll() + diff;
                    if roll <= 15.0 {
                        self.bmsg("Your tank suffers minor damage but is otherwise unharmed.", None);
                        continue;
                    } else if roll <= 25.0 {
                        if self.su(i).immobilized {
                            self.bmsg("Your tank suffers minor damage but is otherwise unharmed.", None);
                        } else {
                            self.bmsg("The hit damages the engine and drivetrain, immobilizing your tank.", None);
                            self.add_journal("Tank was immobilized");
                            self.immobilize_me(i);
                        }
                        continue;
                    } else if roll <= 45.0 {
                        let loc = p.location.clone().unwrap_or_default();
                        self.bmsg(&format!("The hit shatters the armour plate, sending shards of hot metal into the {loc}"), None);
                        self.add_journal("Suffered spalling from a hit");
                        for n in 0..self.su(i).positions.len() {
                            if self.su(i).positions[n].crewman.is_none() {
                                continue;
                            }
                            if let Some(l) = &self.su(i).positions[n].location {
                                if *l != loc {
                                    continue;
                                }
                            }
                            self.resolve_crew_attack(i, n, &CrewAttack { spalling: true, ..CrewAttack::default() }, true);
                        }
                        continue;
                    }
                }
                if ["Vehicle", "Train Car"].contains(&self.su(i).category()) {
                    crate::sound::play("vehicle_explosion_00");
                }
                let mut text = if i == pl { "You were".to_string() } else { format!("{} was", self.uname(i)) };
                text += " destroyed by ";
                text += &if p.attacker == pl { "you.".to_string() } else { format!("{}.", self.uname(p.attacker)) };
                self.bmsg(&text, None);
                if p.attacker == pl {
                    let n = self.uname(i);
                    self.add_journal(&format!("Destroyed a {n}"));
                }
                let loc = p.location.clone();
                self.destroy_me(i, loc, Some((p.attacker, p.weapon)), false);
                break;
            }
        }
        self.sum(i).battle.ap_hits_to_resolve.clear();
    }

    /// `ResolveFP`
    pub fn resolve_fp(&mut self, i: usize) {
        if !self.su(i).alive || self.su(i).battle.fp_to_resolve == 0 {
            return;
        }
        self.move_to_top_of_stack(i);
        let fp = self.su(i).battle.fp_to_resolve;
        let name = self.uname(i);
        let h = (self.su(i).hx, self.su(i).hy);
        self.bmsg(&format!("Resolving {fp} firepower on {name}."), Some(h));
        let (mut destroy, mut rout, mut reduction) = (0.0, 0.0, 0.0);
        let u = self.su(i).clone();
        if ["Vehicle", "Train Car"].contains(&u.category()) {
            if !u.has("armour") {
                for pair in engine("VEH_FP_TK").as_array().into_iter().flatten() {
                    if int(&pair[0]) <= fp {
                        destroy = num(&pair[1]);
                        break;
                    }
                }
            }
        } else {
            destroy = num(engine("RESOLVE_FP_BASE_CHANCE"));
            let (step, m) = (num(engine("RESOLVE_FP_CHANCE_STEP")), num(engine("RESOLVE_FP_CHANCE_MOD")));
            for n in 2..=fp {
                destroy += step * m.powi(n - 1);
            }
            if u.battle.unit_fatigue > 0 {
                destroy += u.battle.unit_fatigue as f64 * 5.0;
            }
            destroy = restrict(destroy);
            if u.reduced {
                destroy = restrict(destroy * 1.2);
            } else {
                reduction = round1(destroy * 0.85);
            }
            if !u.routed {
                rout = reduction * 0.5;
                if u.battle.fortified {
                    rout -= 15.0;
                } else if u.battle.entrenched {
                    rout -= 10.0;
                } else if u.battle.dug_in {
                    rout -= 5.0;
                } else if ["Wooden Buildings", "Woods"].contains(&u.terrain.as_deref().unwrap_or("")) {
                    rout -= 10.0;
                }
                rout = round1(rout.max(0.0));
            } else {
                destroy = restrict(destroy * 3.0);
            }
            if destroy + rout + reduction > 100.0 && rout > 0.0 {
                rout = (100.0 - destroy - reduction).max(0.0);
            }
            if destroy + rout + reduction > 100.0 && reduction > 0.0 {
                reduction = (100.0 - destroy - rout).max(0.0);
            }
            destroy = round1(destroy);
            reduction = round1(reduction);
            rout = round1(rout);
            self.sum(i).battle.unit_fatigue += 1;
        }
        let mut outcome = String::new();
        if destroy + rout + reduction > 0.0 {
            self.show_fp_window(i, fp, destroy, reduction, rout, None);
            let roll = self.roll();
            outcome = if roll <= destroy {
                self.add_journal(&format!("{name} was destroyed."));
                "Destroyed"
            } else if roll <= destroy + reduction {
                self.add_journal(&format!("{name} was reduced."));
                "Reduced"
            } else if roll <= destroy + reduction + rout {
                self.add_journal(&format!("{name} was routed."));
                "Routed"
            } else if ["Gun", "Infantry"].contains(&u.category()) && !u.pinned {
                self.pin_test(i, fp as f64);
                if self.su(i).pinned { "Pinned" } else { "No effect" }
            } else {
                "No effect"
            }
            .into();
            self.show_fp_window(i, fp, destroy, reduction, rout, Some((roll, outcome.clone())));
        }
        match outcome.as_str() {
            "Destroyed" => self.destroy_me(i, None, None, false),
            "Reduced" => self.reduce_me(i),
            "Routed" => self.rout_me(i),
            _ => {}
        }
        if i == self.pl() {
            let mut any = false;
            for n in 0..self.su(i).positions.len() {
                if self.su(i).positions[n].crewman.is_none() {
                    continue;
                }
                let attack = CrewAttack { firepower: Some(fp), ..CrewAttack::default() };
                if !self.resolve_crew_attack(i, n, &attack, true).is_none() {
                    any = true;
                }
            }
            if !any {
                let h = (self.su(i).hx, self.su(i).hy);
                self.bmsg("No Effect", Some(h));
            }
        }
        self.sum(i).battle.fp_to_resolve = 0;
    }

    /// `MoraleCheck`
    pub fn unit_morale_check(&mut self, i: usize, modifier: f64) -> bool {
        let u = self.su(i);
        let mut chance = num(engine("MORALE_CHECK_BASE_CHANCE")) + modifier;
        if u.battle.fortified {
            chance += 50.0;
        } else if u.battle.entrenched {
            chance += 30.0;
        } else if u.battle.dug_in {
            chance += 20.0;
        } else if ["Wooden Buildings", "Woods"].contains(&u.terrain.as_deref().unwrap_or("")) {
            chance += 15.0;
        }
        self.roll() <= restrict(chance)
    }

    /// `PinTest`
    pub fn pin_test(&mut self, i: usize, fp: f64) {
        let u = self.su(i);
        if !["Infantry", "Gun"].contains(&u.category()) || u.pinned {
            return;
        }
        let mut chance = fp * 7.0;
        if u.battle.fortified {
            chance -= 35.0;
        } else if u.battle.entrenched {
            chance -= 20.0;
        } else if u.battle.dug_in {
            chance -= 15.0;
        } else if ["Wooden Buildings", "Woods"].contains(&u.terrain.as_deref().unwrap_or("")) {
            chance -= 10.0;
        }
        if self.roll() <= restrict(chance) {
            self.sum(i).pinned = true;
            self.clear_acquired_targets(i, false);
        }
    }

    pub fn reduce_me(&mut self, i: usize) {
        if ["Gun", "Infantry"].contains(&self.su(i).category()) {
            self.sum(i).reduced = true;
        }
    }

    pub fn rout_me(&mut self, i: usize) {
        let u = self.sum(i);
        u.routed = true;
        u.pinned = false;
    }

    /// `ImmobilizeMe`
    pub fn immobilize_me(&mut self, i: usize) {
        if self.su(i).category() != "Vehicle" {
            return;
        }
        let u = self.sum(i);
        u.moving = false;
        u.immobilized = true;
        u.bogged = false;
    }

    /// `DestroyMe`; `by` the weapon of the knock-out hit, for the bail-out.
    pub fn destroy_me(&mut self, i: usize, location: Option<String>, by: Option<(usize, usize)>, no_vp: bool) {
        self.sum(i).alive = false;
        let h = (self.su(i).hx, self.su(i).hy);
        {
            let s = self.scm();
            if s.on_map(h) {
                s.stack_mut(h).retain(|&x| x != i);
            }
            s.order.retain(|&x| x != i);
        }
        if self.sc().squad.contains(&i) {
            self.scm().squad.retain(|&x| x != i);
            let id = self.su(i).unit_id.clone();
            self.add_journal(&format!("A {id} tank from our squad was knocked out"));
        }
        let pl = self.pl();
        for w in &mut self.sum(pl).weapons {
            if w.selected_target == Some(i) {
                w.selected_target = None;
            }
        }
        self.scm().target_list.retain(|&x| x != i);
        let u = self.su(i).clone();
        if u.owning_player == 1 {
            if no_vp {
                return;
            }
            let mut vp = match u.category() {
                "Infantry" => {
                    self.add_record("Infantry Destroyed", 1);
                    1
                }
                "Gun" => {
                    self.add_record("Guns Destroyed", 1);
                    2
                }
                "Vehicle" => {
                    self.add_record("Vehicles Destroyed", 1);
                    match u.stat("class") {
                        Some("Truck" | "Tankette") => 1,
                        Some("Heavy Tank") => 4,
                        _ => 3,
                    }
                }
                _ => 3,
            };
            let class = u.stat("class").unwrap_or("").to_string();
            if let Some(t) = &u.battle.transport {
                vp += 1;
                self.bmsg(&format!("{class} was transporting a {t}, now destroyed."), Some(h));
            } else if let Some(c) = &u.battle.cargo {
                vp += match c.as_str() {
                    "Ammo" => 2,
                    "Supplies" => 1,
                    _ => 0,
                };
                self.bmsg(&format!("{class} was transporting {c}, now destroyed."), Some(h));
            }
            if self.cd().mission == "Fighting Withdrawal" {
                vp += 1;
            }
            if self.camp().check_national_skill("Defensive Strategy") {
                vp += 1;
            }
            self.award_vp(vp);
        } else if i == pl {
            self.add_journal(&format!("Our {} tank was knocked out", u.unit_id));
            self.scm().finished = true;
            let close_combat = by.is_some_and(|(a, w)| self.su(a).weapons[w].stat("type") == Some("Close Combat"));
            let flame = by.is_some_and(|(a, w)| self.su(a).weapons[w].name() == "Flame Thrower");
            self.player_bail_out(location, close_combat, flame, false);
        }
    }

    /// `DoRecoveryRoll`
    pub fn do_recovery_roll(&mut self, i: usize) {
        if !self.su(i).pinned || !self.unit_morale_check(i, 0.0) {
            return;
        }
        self.sum(i).pinned = false;
        let name = self.uname(i);
        let h = (self.su(i).hx, self.su(i).hy);
        self.bmsg(&format!("{name} is no longer Pinned."), Some(h));
    }

    // ---------------------------------------------------------------------
    // the player's commands

    /// `SelectWeapon`
    pub fn select_weapon(&mut self, forward: bool) {
        let n = self.su(self.pl()).weapons.len();
        if n == 0 {
            return;
        }
        let s = self.scm();
        s.selected_weapon = Some(match s.selected_weapon {
            None => 0,
            Some(i) if forward => (i + 1) % n,
            Some(i) => (i + n - 1) % n,
        });
    }

    /// `BuildTargetList`
    pub fn build_target_list(&mut self) {
        let s = self.sc();
        let list: Vec<usize> = s
            .order
            .iter()
            .copied()
            .filter(|&u| s.units[u].owning_player != 0 && distance(0, 0, s.units[u].hx, s.units[u].hy) <= 3)
            .collect();
        let pl = s.player;
        self.scm().target_list = list.clone();
        for w in &mut self.sum(pl).weapons {
            if w.selected_target.is_some_and(|t| !list.contains(&t)) {
                w.selected_target = None;
            }
        }
    }

    /// `CycleTarget`
    pub fn cycle_target(&mut self, forward: bool) {
        let Some(w) = self.sc().selected_weapon else { return };
        let list = self.sc().target_list.clone();
        if list.is_empty() {
            return;
        }
        let pl = self.pl();
        let cur = self.su(pl).weapons[w].selected_target;
        let next = match cur.and_then(|c| list.iter().position(|&x| x == c)) {
            None if cur.is_none() => {
                self.sum(pl).weapons[w].selected_target = Some(list[0]);
                return;
            }
            None => list[0],
            Some(i) if forward => list[(i + 1) % list.len()],
            Some(i) => list[(i + list.len() - 1) % list.len()],
        };
        self.sum(pl).weapons[w].selected_target = Some(next);
        self.move_to_top_of_stack(next);
    }

    /// `MovePlayer`: forward, back, or (`reposition`) within the hex.
    pub fn move_player(&mut self, forward: bool, reposition: bool) {
        let pl = self.pl();
        if !reposition {
            let double = self.su(pl).terrain.as_deref().and_then(terrain_effects).is_some_and(|e| e.contains_key("Double Bog Check"));
            if double {
                self.do_bog_check(pl, forward, false, false);
            }
            if self.su(pl).bogged {
                self.bmsg("Your tank has becomed bogged.", None);
                self.scm().advance_phase = true;
                return;
            }
        }
        let squad = self.sc().squad.clone();
        let u = self.su(pl).clone();
        crate::sound::play_movement(&u, &mut self.rng);
        self.sum(pl).moving = true;
        self.clear_acquired_targets(pl, false);
        for &u in &squad {
            self.sum(u).moving = true;
            self.clear_acquired_targets(u, false);
        }
        if !reposition {
            let b = self.su(pl).battle.clone();
            let mut chance = if forward { b.forward_move_chance } else { b.reverse_move_chance };
            let roll = self.roll();
            let pu = self.su(pl);
            for pos in ["Commander", "Commander/Gunner"] {
                let Some(c) = pu.crewman_in(pos) else { continue };
                if c.current_cmd == "Direct Movement" {
                    chance += c.skill_mod(if c.ce { 4.0 } else { 2.0 });
                    if c.skills.iter().any(|s| s == "Driver Direction") {
                        chance += c.skill_mod(3.0);
                    }
                    if forward && c.skills.iter().any(|s| s == "Forward!") {
                        chance += c.skill_mod(10.0);
                    }
                }
                break;
            }
            if let Some(c) = pu.crewman_in("Driver") {
                if c.skills.iter().any(|s| s == "Quick Shifter") {
                    chance += c.skill_mod(5.0);
                }
            }
            if roll > chance {
                let bonus = num(engine("BASE_MOVE_BONUS"));
                let b = &mut self.sum(pl).battle;
                if forward {
                    b.reverse_move_bonus = 0.0;
                    b.forward_move_bonus += bonus;
                } else {
                    b.forward_move_bonus = 0.0;
                    b.reverse_move_bonus += bonus;
                }
                self.bmsg("You move but not far enough to enter a new map hex", None);
                for u in std::iter::once(pl).chain(squad.iter().copied()) {
                    self.generate_terrain(u);
                    self.generate_unit_los(u);
                    self.check_for_hd(u, false);
                }
                self.scm().advance_phase = true;
                return;
            }
        }
        if self.breakdown_check(pl) {
            self.bmsg("Your vehicle stalls for a moment, making you unable to move further this turn.", None);
            self.scm().advance_phase = true;
            return;
        }
        if !reposition {
            let b = &mut self.sum(pl).battle;
            b.forward_move_bonus = 0.0;
            b.reverse_move_bonus = 0.0;
            let dir = if forward { 3 } else { 0 };
            let mut moves: Vec<(usize, (i32, i32), (i32, i32))> = Vec::new();
            for &u in &self.sc().order.clone() {
                if u == pl || squad.contains(&u) {
                    continue;
                }
                let (hx, hy) = (self.su(u).hx, self.su(u).hy);
                let (mut nx, mut ny) = hexes::adjacent(hx, hy, dir);
                if distance(0, 0, nx, ny) > 3 {
                    continue;
                }
                if (nx, ny) == (0, 0) {
                    (nx, ny) = hexes::adjacent(0, 0, dir);
                }
                moves.push((u, (hx, hy), (nx, ny)));
            }
            self.animate_unit_moves(&moves);
            for (u, from, to) in moves {
                let s = self.scm();
                s.stack_mut(from).retain(|&x| x != u);
                s.units[u].hx = to.0;
                s.units[u].hy = to.1;
                s.stack_mut(to).push(u);
            }
        } else {
            self.bmsg("You move to a new location within the same hex.", None);
        }
        for u in std::iter::once(pl).chain(squad.iter().copied()) {
            self.generate_terrain(u);
            self.generate_unit_los(u);
            self.check_for_hd(u, false);
            self.set_smoke_level(u);
        }
        self.calculate_move_chances(pl);
        self.do_bog_check(pl, forward, false, reposition);
        if self.su(pl).bogged {
            self.bmsg("Your tank has becomed bogged.", None);
            self.scm().advance_phase = true;
            return;
        }
        if self.extra_move_check(pl) {
            self.bmsg("You have moved swiftly enough to take another move action.", None);
            return;
        }
        self.scm().advance_phase = true;
    }

    /// `PivotPlayer`: the map turns around the player.
    pub fn pivot_player(&mut self, clockwise: bool) {
        let (r, f) = if clockwise { (5, -1) } else { (1, 1) };
        let pl = self.pl();
        let squad = self.sc().squad.clone();
        for u in self.sc().order.clone() {
            if u == pl || squad.contains(&u) {
                continue;
            }
            let (hx, hy) = (self.su(u).hx, self.su(u).hy);
            let to = hexes::rotate(hx, hy, r);
            let s = self.scm();
            s.stack_mut((hx, hy)).retain(|&x| x != u);
            s.stack_mut(to).push(u);
            let unit = &mut s.units[u];
            unit.hx = to.0;
            unit.hy = to.1;
            unit.facing = unit.facing.map(|d| constrain_dir(d + f));
            unit.turret_facing = unit.turret_facing.map(|d| constrain_dir(d + f));
            for d in &mut unit.hull_down {
                *d = constrain_dir(*d + f);
            }
        }
        for d in &mut self.sum(pl).hull_down {
            *d = constrain_dir(*d + f);
        }
        let s = self.scm();
        s.player_pivot = constrain_dir(s.player_pivot + f);
    }

    /// `RotatePlayerTurret`
    pub fn rotate_player_turret(&mut self, clockwise: bool) {
        let pl = self.pl();
        let Some(tf) = self.su(pl).turret_facing else { return };
        self.sum(pl).turret_facing = Some(constrain_dir(tf + if clockwise { 1 } else { -1 }));
        for w in 0..self.su(pl).weapons.len() {
            if self.su(pl).weapons[w].stat("mount") == Some("Turret") {
                self.update_covered_hexes(pl, w);
            }
        }
    }

    // ---------------------------------------------------------------------
    // support and random events

    /// `ResolveSupportRequests`
    pub fn resolve_support_requests(&mut self) {
        if self.cd().air_support_request {
            let mut granted = self.roll() <= self.cd().air_support_level;
            if self.cd().weather.cloud_cover == "Overcast" {
                self.bmsg("Cannot offer air attack support - cloud cover too heavy.", None);
                granted = false;
            }
            if granted {
                let cd = self.cdm();
                cd.air_support_level = (cd.air_support_level - 10.0).max(0.0);
                self.do_air_attack(false);
            } else {
                self.bmsg("Unable to provide air support at this time.", None);
            }
        }
        if self.cd().arty_support_request {
            let mut roll = self.roll();
            if self.camp().check_national_skill("Centralized Fire") {
                roll -= 10.0;
            }
            if roll <= self.cd().arty_support_level {
                let cd = self.cdm();
                cd.arty_support_level = (cd.arty_support_level - 10.0).max(0.0);
                self.do_artillery_attack(false);
            } else {
                self.bmsg("Unable to provide artillery support at this time.", None);
            }
        }
        let cd = self.cdm();
        cd.air_support_request = false;
        cd.arty_support_request = false;
    }

    /// "You were" or "X was".
    fn was(&self, i: usize) -> String {
        if i == self.pl() { "You were".into() } else { format!("{} was", self.uname(i)) }
    }

    /// `DoAirAttack`
    pub fn do_air_attack(&mut self, player_target: bool) {
        let friendly = if player_target { 1 } else { 0 };
        self.bmsg("Air support attack inbound! Trying to spot targets.", None);
        let mut chance = 55.0;
        match self.cd().weather.cloud_cover.as_str() {
            "Scattered" => chance -= 10.0,
            "Heavy" => chance -= 20.0,
            _ => {}
        }
        match self.cd().weather.precipitation.as_str() {
            "Rain" | "Snow" => chance -= 15.0,
            "Heavy Rain" | "Blizzard" => chance -= 25.0,
            _ => {}
        }
        let mut targets: Vec<(i32, i32)> = Vec::new();
        for n in 0..self.sc().hexes.len() {
            let (h, stack) = (self.sc().hexes[n], self.sc().stacks[n].clone());
            if stack.is_empty() || self.su(stack[0]).owning_player == friendly {
                continue;
            }
            for u in stack {
                let x = self.su(u);
                let mut m = 0.0;
                if x.smoke > 0 {
                    m -= 5.0;
                }
                match x.stat("size_class") {
                    Some("Small") => m -= 7.0,
                    Some("Very Small") => m -= 18.0,
                    Some("Large") => m += 7.0,
                    Some("Very Large") => m += 18.0,
                    _ => {}
                }
                if x.moving {
                    m += 10.0;
                }
                m += self.tem(u);
                if self.roll() > restrict(chance + (m * 0.25 * 100.0).round() / 100.0) {
                    continue;
                }
                targets.push(h);
                break;
            }
        }
        if targets.is_empty() {
            self.bmsg("No targets spotted, ending attack.", None);
            return;
        }
        let roll = self.rng.int(1, 10);
        let planes = if roll <= 5 {
            1
        } else if roll <= 8 {
            2
        } else {
            3
        };
        let key = if player_target { "enemy_air_support" } else { "player_air_support" };
        let ids = defs::strings(&self.camp().stats[key]);
        let plane_id = self.rng.choice(&ids).clone();
        self.bmsg(&format!("{planes} {plane_id} arrive{} for an attack.", if planes == 1 { "s" } else { "" }), None);
        let mut plane_units = Vec::new();
        for _ in 0..planes {
            if let Some(u) = Unit::new(&plane_id, &mut self.rng) {
                plane_units.push(self.add_unit(u));
            }
        }
        let Some(&first) = plane_units.first() else { return };
        let bomb_w = self.su(first).weapons.iter().position(|w| w.name() == "Bombs").unwrap_or(0);
        let bomb_cal = self.su(first).weapons.get(bomb_w).and_then(|w| w.stats.get("calibre")).map(int).unwrap_or(0);
        let mut fp = 0;
        for pair in engine("HE_FP_EFFECT").as_array().into_iter().flatten() {
            fp = int(&pair[1]);
            if int(&pair[0]) <= bomb_cal {
                break;
            }
        }
        let effective_fp = fp / 2;
        let mut dirs = Vec::new();
        for &h in &targets {
            let d = self.animate_air_attack(h);
            dirs.push(d);
        }
        let mut results = false;
        for (n, &h) in targets.iter().enumerate() {
            let direction = dirs[n];
            for &plane in &plane_units {
                let stack = self.sc().stack(h).clone();
                if stack.is_empty() {
                    continue;
                }
                let target = *self.rng.choice(&stack);
                let t = self.su(target);
                let pf = engine("PF_BASE_CHANCE");
                let mut chance = if !t.spotted || t.category() != "Vehicle" { num(&pf[0][1]) } else { num(&pf[0][0]) };
                if let Some(sc) = t.stat("size_class") {
                    if sc != "Normal" {
                        chance += engine("PF_SIZE_MOD").get(sc).map(num).unwrap_or(0.0);
                    }
                }
                if t.smoke >= 2 {
                    chance -= 30.0;
                } else if t.smoke == 1 {
                    chance -= 15.0;
                }
                let chance = restrict((chance / 2.0).trunc());
                let (thx, thy) = (t.hx, t.hy);
                if self.roll() > chance {
                    continue;
                }
                results = true;
                {
                    let p = self.sum(plane);
                    p.hx = thx;
                    p.hy = thy - direction;
                }
                let direct = self.roll() <= num(engine("DIRECT_HIT_CHANCE"));
                let cat = self.su(target).category().to_string();
                if cat == "Infantry" || cat == "Gun" {
                    if direct {
                        let text = format!("{} destroyed by a direct hit from the air attack!", self.was(target));
                        self.bmsg(&text, Some((thx, thy)));
                        self.destroy_me(target, None, None, false);
                        continue;
                    }
                    let t = self.sum(target);
                    t.battle.fp_to_resolve += effective_fp;
                    if !t.spotted {
                        t.battle.hit_by_fp = true;
                    }
                    let text = format!("{} hit by the air attack.", self.was(target));
                    self.bmsg(&text, Some((thx, thy)));
                    self.resolve_fp(target);
                } else if cat == "Vehicle" {
                    let t = self.su(target);
                    if direct && (!t.has("armour") || t.has("open_topped")) {
                        let text = format!("{} destroyed by a direct hit from the air attack.", self.was(target));
                        self.bmsg(&text, Some((thx, thy)));
                        self.destroy_me(target, None, None, false);
                        continue;
                    }
                    let loc = if self.roll() <= 50.0 { "Hull" } else { "Turret" };
                    let p = Profile {
                        attacker: first,
                        weapon: bomb_w,
                        ammo_type: Some("HE".into()),
                        target,
                        location: Some(loc.into()),
                        ..Profile::default()
                    };
                    let mut p = self.calc_ap(p, true);
                    if direct {
                        p.final_chance = round1(p.final_chance * 2.0).min(100.0);
                    }
                    if self.roll() > p.final_chance {
                        let text = format!("{} unaffected by the air attack", self.was(target));
                        self.bmsg(&text, Some((thx, thy)));
                        continue;
                    }
                    let text = format!("{} destroyed by the air attack.", self.was(target));
                    self.bmsg(&text, Some((thx, thy)));
                    self.destroy_me(target, None, None, false);
                }
            }
        }
        if !results {
            self.bmsg("Air attack had no effect.", None);
        }
    }

    /// `DoArtilleryAttack`
    pub fn do_artillery_attack(&mut self, player_target: bool) {
        let friendly = if player_target { 1 } else { 0 };
        self.bmsg("Artillery support attack inbound! Trying to spot targets.", None);
        let mut targets = Vec::new();
        for n in 0..self.sc().hexes.len() {
            let stack = &self.sc().stacks[n];
            if stack.is_empty() || self.su(stack[0]).owning_player == friendly {
                continue;
            }
            targets.push(self.sc().hexes[n]);
        }
        if targets.is_empty() {
            self.bmsg("No targets spotted, ending attack.", None);
            return;
        }
        self.bmsg("Target spotted, firing for effect.", None);
        for &h in &targets {
            self.animate_bombardment(h);
        }
        let key = if player_target { "enemy_arty_support" } else { "player_arty_support" };
        let ids = defs::strings(&self.camp().stats[key]);
        let id = self.rng.choice(&ids).clone();
        let Some(gun) = Unit::new(&id, &mut self.rng) else { return };
        let gun_cal = gun.weapons.first().and_then(|w| w.stats.get("calibre")).map(int).unwrap_or(0);
        let gun = self.add_unit(gun);
        let mut fp = 0;
        for pair in engine("HE_FP_EFFECT").as_array().into_iter().flatten() {
            fp = int(&pair[1]);
            if int(&pair[0]) <= gun_cal {
                break;
            }
        }
        let effective_fp = fp / 2;
        let (mut skill_mod, mut soft_mod) = (0.0, 0.0);
        let pu = self.camp().pu();
        for p in &pu.positions {
            let Some(c) = &p.crewman else { continue };
            if player_positions().contains(&p.name) {
                if c.skills.iter().any(|s| s == "Centralized Fire") {
                    skill_mod = -15.0;
                }
                if c.skills.iter().any(|s| s == "Combined Bombardment") {
                    soft_mod = -25.0;
                }
                break;
            }
        }
        let mut results = false;
        for &h in &targets {
            for target in self.sc().stack(h).clone() {
                let t = self.su(target);
                // the original passes over the player's side here even when
                // the enemy is firing at it, so enemy artillery never
                // harms the player (kept)
                if t.owning_player == 0 || !t.alive {
                    continue;
                }
                let mut chance = if t.category() == "Vehicle" { num(engine("VEH_FP_BASE_CHANCE")) } else { num(engine("INF_FP_BASE_CHANCE")) };
                let (step, m) = (num(engine("FP_CHANCE_STEP")), num(engine("FP_CHANCE_STEP_MOD")));
                for n in 2..=effective_fp {
                    chance += step * m.powi(n - 1);
                }
                let chance = restrict((chance / 2.0).trunc());
                let cat = t.category().to_string();
                let (thx, thy) = (t.hx, t.hy);
                let mut roll = self.roll() + skill_mod;
                if cat == "Infantry" || cat == "Gun" {
                    roll += soft_mod;
                }
                if roll > chance {
                    continue;
                }
                results = true;
                {
                    let g = self.sum(gun);
                    g.hx = thx;
                    g.hy = thy + 4;
                }
                let direct = self.roll() <= num(engine("DIRECT_HIT_CHANCE"));
                if cat == "Infantry" || cat == "Gun" {
                    if direct {
                        let text = format!("{} destroyed by a direct hit from the artillery attack.", self.was(target));
                        self.bmsg(&text, Some((thx, thy)));
                        self.destroy_me(target, None, None, false);
                        continue;
                    }
                    let t = self.sum(target);
                    t.battle.fp_to_resolve += effective_fp;
                    if !t.spotted {
                        t.battle.hit_by_fp = true;
                    }
                    let text = format!("{} hit by artillery attack.", self.was(target));
                    self.bmsg(&text, Some((thx, thy)));
                    self.resolve_fp(target);
                } else if cat == "Vehicle" {
                    let t = self.su(target);
                    if direct && (!t.has("armour") || t.has("open_topped")) {
                        let text = format!("{} destroyed by a direct hit from the artillery attack.", self.was(target));
                        self.bmsg(&text, Some((thx, thy)));
                        self.destroy_me(target, None, None, false);
                        continue;
                    }
                    let loc = if self.roll() <= 50.0 { "Hull" } else { "Turret" };
                    let p = Profile { attacker: gun, weapon: 0, ammo_type: Some("HE".into()), target, location: Some(loc.into()), ..Profile::default() };
                    let mut p = self.calc_ap(p, false);
                    if direct {
                        p.final_chance = round1(p.final_chance * 2.0).min(100.0);
                    }
                    if self.roll() > p.final_chance {
                        let text = if target == self.pl() {
                            "You were hit by the artillery attack but remain unharmed.".to_string()
                        } else {
                            format!("{} was hit by the artillery attack but remains unharmed.", self.uname(target))
                        };
                        self.bmsg(&text, Some((thx, thy)));
                        let t = self.sum(target);
                        if !t.spotted {
                            t.battle.hit_by_fp = true;
                        }
                        continue;
                    }
                    let text = format!("{} destroyed by artillery attack", self.was(target));
                    self.bmsg(&text, Some((thx, thy)));
                    self.destroy_me(target, None, None, false);
                }
            }
        }
        if !results {
            self.bmsg("Artillery attack had no effect.", None);
        }
    }

    /// `Scenario.CheckForRandomEvent`
    pub fn check_for_battle_random_event(&mut self) {
        if self.roll() > self.sc().random_event_chance {
            self.scm().random_event_chance += 1.0;
            return;
        }
        let roll = self.roll();
        let week = self.camp().week().clone();
        let stats = self.camp().stats.clone();
        let pl = self.pl();
        if roll <= 10.0 {
            if !week.contains_key("air_support_level") || self.cd().weather.cloud_cover == "Overcast" {
                return;
            }
            self.bmsg("Friendly air forces launch an attack!", None);
            self.do_air_attack(false);
        } else if roll <= 20.0 {
            if !week.contains_key("arty_support_level") {
                return;
            }
            self.bmsg("Friendly artillery forces fire a bombardment!", None);
            self.do_artillery_attack(false);
        } else if roll <= 30.0 {
            let n = self.sc().enemy_reinforcements;
            if n > 0 && self.roll() <= n as f64 * 40.0 {
                return;
            }
            self.scm().enemy_reinforcements += 1;
            self.spawn_enemy_units(true);
            self.bmsg("Enemy reinforcements have arrived!", None);
        } else if roll <= 40.0 {
            let s = self.sc();
            let list: Vec<usize> = s
                .order
                .iter()
                .copied()
                .filter(|&u| {
                    let x = &s.units[u];
                    x.alive && x.owning_player != 0 && !["Gun", "Train Car"].contains(&x.category()) && x.battle.ai.as_ref().is_some_and(|a| !a.recall)
                })
                .collect();
            if list.is_empty() {
                return;
            }
            let u = *self.rng.choice(&list);
            if let Some(ai) = &mut self.sum(u).battle.ai {
                ai.recall = true;
            }
            if self.su(u).spotted {
                let text = format!("{} is being recalled from the battle.", self.uname(u));
                let h = (self.su(u).hx, self.su(u).hy);
                self.bmsg(&text, Some(h));
            }
        } else if roll <= 50.0 {
            let crew = self.vulnerable_crew(pl);
            if crew.is_empty() {
                return;
            }
            let target = *self.rng.choice(&crew);
            let mut chance = num(engine("BASE_SNIPER_TK_CHANCE"));
            // the original compares the precipitation with a list for the
            // heavier kinds, never equal (kept)
            if ["Rain", "Light Snow"].contains(&self.cd().weather.precipitation.as_str()) {
                chance -= 5.0;
            }
            if self.su(pl).moving {
                chance -= 10.0;
            }
            chance += self.tem(pl);
            if chance < 15.0 {
                return;
            }
            let roll = self.roll();
            // the original compares the position itself with the player
            // positions' names, never equal, so fate points never turn a
            // sniper away and the message always names the position (kept)
            let pos = self.su(pl).positions[target].name.clone();
            if roll > chance {
                crate::sound::play("ricochet");
                self.battle_message(&format!("The ricochet from a sniper's bullet rings out, narrowly missing your {pos}!"), true, None, None);
            } else {
                crate::sound::play("sniper_hit");
                self.battle_message(&format!("Your {pos} has been hit by a sniper!"), true, None, None);
                self.resolve_crew_attack(pl, target, &CrewAttack { sniper: true, ..CrewAttack::default() }, true);
            }
        } else if roll <= 60.0 {
            let s = self.sc();
            let list: Vec<usize> = s.order.iter().copied().filter(|&u| s.units[u].owning_player != 0 && s.units[u].has("armour")).collect();
            if list.is_empty() {
                return;
            }
            let u = *self.rng.choice(&list);
            self.immobilize_me(u);
            if self.su(u).spotted {
                let text = format!("{} has been immobilized!", self.uname(u));
                let h = (self.su(u).hx, self.su(u).hy);
                self.bmsg(&text, Some(h));
            }
        } else if roll <= 70.0 {
            let s = self.sc();
            let list: Vec<usize> = s.order.iter().copied().filter(|&u| s.units[u].owning_player != 0 && !s.units[u].spotted).collect();
            if list.is_empty() {
                return;
            }
            let u = *self.rng.choice(&list);
            self.sum(u).spotted = true;
            let text = format!("{} spotted!", self.uname(u));
            let x = self.su(u);
            let portrait = x.stat("portrait").map(str::to_string);
            let h = (x.hx, x.hy);
            self.battle_message(&text, false, portrait.as_deref(), Some(h));
        } else if roll <= 75.0 {
            if !stats.contains_key("enemy_air_support") || self.cd().weather.cloud_cover == "Overcast" {
                return;
            }
            self.bmsg("Enemy air forces launch an attack!", None);
            self.do_air_attack(true);
        } else if roll <= 80.0 {
            if !stats.contains_key("enemy_arty_support") {
                return;
            }
            self.bmsg("Enemy artillery forces fire a bombardment!", None);
            self.do_artillery_attack(true);
        } else {
            return;
        }
        self.scm().random_event_chance = num(engine("BASE_RANDOM_EVENT_CHANCE"));
    }

    // ---------------------------------------------------------------------
    // the bail-out

    /// `PlayerBailOut`: the crew get out of a knocked-out (or abandoned)
    /// tank, on its own screen.
    pub fn player_bail_out(&mut self, location: Option<String>, close_combat: bool, flame: bool, skip_ko: bool) {
        let pl = self.pl();
        let n = self.su(pl).positions.len();
        for p in 0..n {
            if let Some(c) = &mut self.sum(pl).positions[p].crewman {
                c.bailed_out = false;
            }
        }
        let mut cols: Vec<[String; 3]> = vec![Default::default(); n];
        let mut burn_text = String::new();
        self.draw_bail_out(&cols, &burn_text, false);
        self.wait(120, false);
        if !skip_ko {
            for p in 0..n {
                let Some(c) = &self.su(pl).positions[p].crewman else { continue };
                if !c.alive {
                    continue;
                }
                let attack = CrewAttack { ko_hit: Some((location.clone(), close_combat)), ..CrewAttack::default() };
                let r = self.resolve_crew_attack(pl, p, &attack, false);
                // the original wrapped `False` as text here and crashed
                cols[p][0] = match r {
                    CrewHit::Text(t) => t,
                    _ => "OK".into(),
                };
                self.draw_bail_out(&cols, &burn_text, false);
                self.wait(100, false);
            }
            self.wait(100, false);
        }
        for p in 0..n {
            let Some(c) = self.su(pl).positions[p].crewman.clone() else { continue };
            let pause;
            if !c.alive || c.condition == "Unconscious" {
                cols[p][1] = "N/A".into();
                pause = 10;
            } else {
                pause = 100;
                let mut m = 0.0;
                if c.condition == "Stunned" {
                    m += 5.0;
                }
                for (k, v) in &c.injury {
                    if !k.contains("Leg") {
                        continue;
                    }
                    match v.as_deref() {
                        Some("Heavy" | "Serious") => m += 40.0,
                        Some("Critical") => m += 60.0,
                        _ => {}
                    }
                }
                if !self.su(pl).positions[p].hatch {
                    m += 15.0;
                } else if c.ce {
                    m -= 20.0;
                }
                if c.skills.iter().any(|s| s == "Gymnast") {
                    m -= c.skill_mod(10.0);
                }
                let mut roll = self.roll();
                if roll < 97.0 {
                    roll += m;
                }
                if roll <= 97.0 {
                    self.crew_mut(pl, p).bailed_out = true;
                    cols[p][1] = "Bailed".into();
                } else {
                    cols[p][1] = "Failed".into();
                }
            }
            self.draw_bail_out(&cols, &burn_text, false);
            self.wait(pause, false);
        }
        let mut chance = if self.su(pl).has("wet_stowage") { 25.0 } else { 80.0 };
        if flame {
            chance = 90.0;
        }
        let burns = self.roll() <= chance;
        burn_text = if burns { "Tank Burns".into() } else { "No Burn-up".into() };
        if burns {
            for p in 0..n {
                let Some(c) = &self.su(pl).positions[p].crewman else { continue };
                if c.bailed_out || !c.alive {
                    continue;
                }
                cols[p][2] = "Vulnerable".into();
            }
        }
        self.draw_bail_out(&cols, &burn_text, false);
        self.wait(100, false);
        for p in 0..n {
            let Some(c) = &self.su(pl).positions[p].crewman else { continue };
            if c.bailed_out || !c.alive {
                continue;
            }
            let mut t = "Rescued".to_string();
            if burns {
                if let CrewHit::Text(r) = self.resolve_crew_attack(pl, p, &CrewAttack { burn_up: true, ..CrewAttack::default() }, false) {
                    t = r;
                }
            }
            cols[p][2] = t;
            self.draw_bail_out(&cols, &burn_text, false);
            self.wait(100, false);
        }
        self.draw_bail_out(&cols, &burn_text, true);
        self.wait_for_continue(false);
    }

    // ---------------------------------------------------------------------
    // starting and ending

    /// The original's `Scenario(cd_map_hex)` and the set-up at the start
    /// of `DoScenarioLoop`.
    pub fn start_battle(&mut self, zone: (i32, i32)) {
        if self.scenario.is_none() {
            self.scenario = Some(Scenario::new(zone));
        }
        let mut pu = self.camp().pu().clone();
        pu.reset_me();
        pu.facing = Some(0);
        pu.turret_facing = Some(0);
        for p in &mut pu.positions {
            if let Some(c) = &mut p.crewman {
                c.current_cmd = "Spot".into();
            }
        }
        let pl = self.add_unit(pu);
        self.scm().player = pl;
        self.spawn_at(pl, 0, 0);
        for mut u in std::mem::take(&mut self.cdm().player_squad) {
            u.reset_me();
            u.facing = Some(0);
            u.turret_facing = Some(0);
            let i = self.add_unit(u);
            self.spawn_at(i, 0, 0);
            self.scm().squad.push(i);
        }
        self.spawn_enemy_units(false);
        self.generate_los();
        self.build_cmd_lists(pl);
        self.reset_for_new_turn(pl, true);
        for u in self.sc().squad.clone() {
            self.build_cmd_lists(u);
            self.reset_for_new_turn(u, true);
        }
        if self.cd().hex(zone).controlled_by == 1 {
            self.do_ambush_roll();
        }
        if self.sc().ambush {
            let s = self.scm();
            s.phase = PHASE_ALLIED_ACTION;
            s.advance_phase = true;
        }
        self.scm().init_complete = true;
    }

    /// The battle is over: the player's tank and squad go back to the
    /// campaign (`campaign.player_unit = self.player_unit`).
    pub fn end_battle(&mut self) {
        let Some(s) = self.scenario.take() else { return };
        let mut pu = s.units[s.player].clone();
        pu.battle.los.clear();
        pu.battle.ap_hits_to_resolve.clear();
        self.campm().player_unit = Some(pu);
        let squad: Vec<Unit> = s.squad.iter().map(|&i| s.units[i].clone()).collect();
        let cd = self.cdm();
        cd.player_squad = squad;
        let pu = self.camp().pu().clone();
        crate::cday::build_player_gun_list(&pu, self.cdm());
    }

    // ---------------------------------------------------------------------
    // the turn

    /// `AdvanceToNextPhase`
    pub fn advance_to_next_phase(&mut self) {
        let pl = self.pl();
        let phase = self.sc().phase;
        if phase == PHASE_ALLIED_ACTION {
            self.scm().context_text.clear();
            let active = self.sc().active_player;
            let mut idx = self.sc().order.len() as isize - 1;
            while let Some(u) = self.rev_next(&mut idx) {
                if !self.su(u).alive || self.su(u).owning_player == active {
                    continue;
                }
                self.resolve_fp(u);
            }
            let s = self.scm();
            s.phase = PHASE_ENEMY_ACTION;
            s.active_player = 1;
        } else if phase == PHASE_ENEMY_ACTION {
            self.scm().context_text.clear();
            let active = self.sc().active_player;
            let mut idx = self.sc().order.len() as isize - 1;
            while let Some(u) = self.rev_next(&mut idx) {
                if !self.su(u).alive || self.su(u).owning_player == active {
                    continue;
                }
                self.resolve_fp(u);
            }
            let squad = self.sc().squad.clone();
            for u in std::iter::once(pl).chain(squad.iter().copied()) {
                if !self.sc().order.contains(&u) {
                    continue;
                }
                self.sum(u).battle.overrun = false;
                self.generate_terrain(u);
                self.check_for_hd(u, false);
                self.set_smoke_level(u);
            }
            self.advance_clock(0, int(engine("TURN_LENGTH")), false);
            self.check_for_end();
            if self.sc().finished {
                return;
            }
            self.check_for_battle_random_event();
            if !self.su(pl).alive {
                self.cdm().ended = true;
                return;
            }
            self.check_for_end();
            if self.sc().finished {
                return;
            }
            let s = self.scm();
            s.active_player = 0;
            s.phase = PHASE_COMMAND;
            self.reset_for_new_turn(pl, false);
            for u in self.sc().squad.clone() {
                self.reset_for_new_turn(u, false);
            }
            self.move_to_top_of_stack(pl);
        } else {
            if phase == PHASE_MOVEMENT {
                if self.sc().player_pivot != 0 {
                    self.clear_acquired_targets(pl, true);
                    if !self.su(pl).bogged {
                        self.do_bog_check(pl, false, true, false);
                        if self.su(pl).bogged {
                            self.bmsg("Your tank has becomed bogged.", None);
                        }
                    }
                }
            } else if phase == PHASE_SHOOTING {
                let active = self.sc().active_player;
                let mut idx = self.sc().order.len() as isize - 1;
                while let Some(u) = self.rev_next(&mut idx) {
                    if !self.su(u).alive || self.su(u).owning_player == active {
                        continue;
                    }
                    self.resolve_ap_hits(u);
                }
            }
            if self.scenario.is_none() {
                return;
            }
            self.scm().phase += 1;
        }
        if self.scenario.is_none() || self.sc().finished {
            return;
        }
        self.draw_battle();
        self.flush();

        match self.sc().phase {
            PHASE_COMMAND => self.build_cmd_lists(pl),
            PHASE_SPOTTING => {
                let swap = self.su(pl).positions.iter().filter_map(|p| p.crewman.as_ref()).any(|c| c.current_cmd == "Swap Position");
                if swap {
                    self.show_swap_position_menu();
                }
                self.do_spot_checks(pl);
                self.scm().advance_phase = true;
            }
            PHASE_CREW_ACTION => {
                let mut input = false;
                for p in 0..self.su(pl).positions.len() {
                    let Some(c) = self.su(pl).positions[p].crewman.clone() else { continue };
                    match c.current_cmd.as_str() {
                        "Withdraw" => {
                            if self.show_notification("Withdraw from this battle?", true) {
                                self.cdm().player_withdrew = true;
                                self.scm().finished = true;
                                return;
                            }
                            continue;
                        }
                        "Abandon Tank" => {
                            self.campm().pum().alive = false;
                            self.bmsg("You abandon your tank.", None);
                            self.add_journal("Abandoned tank");
                            self.sum(pl).alive = false;
                            self.player_bail_out(None, false, false, true);
                            let cd = self.cdm();
                            cd.ended = true;
                            cd.abandoned_tank = true;
                            self.scm().finished = true;
                            return;
                        }
                        "Attempt Unbog" => {
                            if self.do_unbog_check(pl) {
                                self.bmsg("Your tank is no longer bogged down.", None);
                            } else {
                                self.bmsg("Your tank remains bogged down.", None);
                            }
                        }
                        "Smoke Grenade" => {
                            self.sum(pl).smoke = 1;
                            crate::sound::play("smoke");
                            self.bmsg("You throw a smoke grenade.", None);
                        }
                        "Fire Smoke Mortar" => {
                            self.sum(pl).smoke = 2;
                            crate::sound::play("smoke");
                            let name = self.su(pl).positions[p].name.clone();
                            self.bmsg(&format!("The {name} fires off a smoke mortar round."), None);
                        }
                        "Manage Ready Rack" => input = true,
                        _ => {}
                    }
                }
                if !input {
                    self.scm().advance_phase = true;
                }
            }
            PHASE_MOVEMENT => {
                self.scm().player_pivot = 0;
                match self.su(pl).crewman_in("Driver").map(|c| c.current_cmd.clone()) {
                    None => self.scm().advance_phase = true,
                    Some(cmd) if cmd == "Drive" => self.calculate_move_chances(pl),
                    Some(cmd) => {
                        self.scm().advance_phase = true;
                        if cmd == "Overrun" {
                            self.bmsg("You move into position for an Overrun attack.", None);
                            self.move_player(false, true);
                            self.sum(pl).battle.overrun = true;
                            for u in self.sc().squad.clone() {
                                self.sum(u).battle.overrun = true;
                            }
                        }
                    }
                }
            }
            PHASE_SHOOTING => {
                let any = self.su(pl).positions.iter().filter_map(|p| p.crewman.as_ref()).any(|c| c.current_cmd == "Operate Gun" || c.current_cmd == "Operate MG");
                if any {
                    self.build_target_list();
                } else {
                    self.scm().advance_phase = true;
                }
            }
            PHASE_ALLIED_ACTION => {
                self.do_ai_spot_checks(0);
                // `for unit in scenario.player_unit.squad`, which squad
                // members leave as they are knocked out
                let mut n = 0;
                while n < self.sc().squad.len() {
                    let u = self.sc().squad[n];
                    n += 1;
                    self.move_to_top_of_stack(u);
                    self.draw_battle();
                    self.ai_activation(u);
                    let active = self.sc().active_player;
                    for u2 in self.sc().order.clone() {
                        if !self.su(u2).alive || self.su(u2).owning_player == active {
                            continue;
                        }
                        self.resolve_ap_hits(u2);
                    }
                    self.do_recovery_roll(u);
                }
                self.scm().advance_phase = true;
            }
            PHASE_ENEMY_ACTION => {
                self.do_ai_spot_checks(1);
                let mut idx = self.sc().order.len() as isize - 1;
                while let Some(u) = self.rev_next(&mut idx) {
                    if !self.su(pl).alive {
                        break;
                    }
                    if self.su(u).owning_player == 0 || !self.su(u).alive {
                        continue;
                    }
                    self.reset_for_new_turn(u, false);
                    self.calculate_move_chances(u);
                    self.ai_activation(u);
                    let active = self.sc().active_player;
                    for u2 in self.sc().order.clone() {
                        if !self.su(u2).alive || self.su(u2).owning_player == active {
                            continue;
                        }
                        self.resolve_ap_hits(u2);
                    }
                    self.do_recovery_roll(u);
                }
                let s = self.scm();
                s.ambush = false;
                s.advance_phase = true;
            }
            _ => {}
        }
    }
}

/// `DoCrewRecoveryCheck`, after a battle: crewmen with critical injuries
/// stabilise or die. The original set the campaign day's condition rather
/// than each crewman's, so crewmen do not recover from Shaken, Stunned or
/// Unconscious here (kept); it also named an unset `location` and called
/// battle code after the battle, and crashed when a crewman died.
impl Game {
    pub fn do_crew_recovery_check(&mut self) {
        if !self.camp().pu().alive || self.camp().ended {
            return;
        }
        let n = self.camp().pu().positions.len();
        for p in 0..n {
            let Some(c) = self.camp().pu().positions[p].crewman.clone() else { continue };
            if !c.alive {
                continue;
            }
            for (k, v) in &c.injury {
                if v.as_deref() != Some("Critical") {
                    continue;
                }
                let roll = percentile(&mut self.rng) - 20.0;
                let pos = self.camp().pu().positions[p].name.clone();
                if roll <= c.stat("Grit") as f64 * 9.0 {
                    if let Some(slot) = self.campm().pum().positions[p].crewman.as_mut().unwrap().injury.iter_mut().find(|(x, _)| x == k) {
                        slot.1 = Some("Serious".into());
                    }
                    continue;
                }
                {
                    let c = self.campm().pum().positions[p].crewman.as_mut().unwrap();
                    c.alive = false;
                    c.condition = "Dead".into();
                    c.fatigue = 0;
                }
                let you = player_positions().contains(&pos);
                let text = if self.camp().options.permadeath && you {
                    let camp = self.campm();
                    camp.ended = true;
                    camp.player_oob = true;
                    self.cdm().ended = true;
                    format!("You succumb to your {k} injury and die. Your campaign is over.")
                } else {
                    format!("Your {pos} succumbed to their {k} injury and has died.")
                };
                self.show_message(&text, false, None);
                self.add_journal(&text);
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearings_match_the_hex_directions() {
        for d in 0..6 {
            let (hx, hy) = hexes::adjacent(0, 0, d);
            assert_eq!(direction_toward(0, 0, hx, hy), d, "{d}");
        }
    }

    #[test]
    fn two_d6() {
        assert_eq!(odds_2d6(7), 58.3);
        assert_eq!(restrict(1.0), 3.0);
        assert_eq!(restrict(99.0), 97.0);
    }
}
