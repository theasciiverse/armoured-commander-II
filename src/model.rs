//! The original's `Personnel`, `Position`, `Weapon` and `Unit` classes:
//! the player's tank and crew, and every other unit on the map.
//!
//! The original links them both ways (a crewman points to his position
//! and his unit, a position to its unit); here a unit owns its positions
//! and each position owns the crewman in it, so a crewman's position is
//! wherever he is kept, and what needs the position or unit takes it.
//! Messages the rules would show are handed back for the caller to show.

use crate::defs::{self, Dict, engine, int, num, text};
use crate::rng::Rng;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const INJURY_LOCATIONS: [&str; 6] =
    ["Head & Neck", "Torso & Groin", "Right Arm & Hand", "Left Arm & Hand", "Right Leg & Foot", "Left Leg & Foot"];

/// `GetPercentileRoll`: 0.0 to 100.0 in tenths.
pub fn percentile(rng: &mut Rng) -> f64 {
    rng.int(0, 1000) as f64 / 10.0
}

/// `GetExpRequiredFor`
pub fn exp_required_for(level: i32) -> i32 {
    let base = num(engine("BASE_EXP_REQUIRED"));
    let exponent = num(engine("EXP_EXPONENT"));
    (1..level).map(|l| (base * (l as f64).powf(exponent)).ceil() as i32).sum()
}

/// `calendar.monthrange(year, month)[1]`
pub fn days_in_month(year: i32, month: i32) -> i32 {
    match month {
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// What a new crewman needs to know of the campaign.
pub struct CrewContext<'a> {
    /// The crewman is in the player's unit (and gets the campaign's
    /// national skills).
    pub player: bool,
    pub national_skills: &'a [String],
    /// `campaign.today`, "YYYY.MM.DD".
    pub today: &'a str,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Personnel {
    pub nation: String,
    /// The position the crewman is normally supposed to be in.
    pub normal_position: String,
    pub alive: bool,
    pub bailed_out: bool,
    pub fatigue: i32,
    pub condition: String,
    /// Injury level per body system, in `INJURY_LOCATIONS` order.
    pub injury: Vec<(String, Option<String>)>,
    /// (days min, days max) if the crewman must go to the field hospital.
    pub field_hospital: Option<(i32, i32)>,
    pub first_name: String,
    pub last_name: String,
    pub nickname: String,
    pub age: i32,
    pub rank: i32,
    /// Perception, Grit, Knowledge, Morale.
    pub stats: BTreeMap<String, i32>,
    pub skills: Vec<String>,
    pub level: i32,
    pub exp: i32,
    pub adv: i32,
    pub birthday: String,
    /// Exposed (crew exposed) rather than buttoned up.
    pub ce: bool,
    pub cmd_list: Vec<String>,
    pub current_cmd: String,
}

impl Personnel {
    /// `Personnel.__init__`, for the given position.
    pub fn new(nation: &str, position: &Position, ctx: &CrewContext, rng: &mut Rng) -> Self {
        let mut p = Personnel {
            nation: nation.to_string(),
            normal_position: position.name.clone(),
            alive: true,
            bailed_out: false,
            fatigue: -5,
            condition: "Good Order".into(),
            injury: INJURY_LOCATIONS.iter().map(|l| (l.to_string(), None)).collect(),
            field_hospital: None,
            first_name: String::new(),
            last_name: String::new(),
            nickname: String::new(),
            age: 20,
            rank: 0,
            stats: ["Perception", "Grit", "Knowledge", "Morale"].iter().map(|s| (s.to_string(), 1)).collect(),
            skills: Vec::new(),
            level: 1,
            exp: 0,
            adv: 1,
            birthday: String::new(),
            ce: false,
            cmd_list: Vec::new(),
            current_cmd: "Spot".into(),
        };
        p.generate_name(rng);

        // randomly increase two stats to 3: `sample(range(3), 2)` of
        // CREW_STATS, which never picks the fourth, Knowledge (kept)
        let crew_stats = defs::strings(engine("CREW_STATS"));
        let first = rng.int(0, 2);
        let second = (first + rng.int(1, 2)) % 3;
        for i in [first, second] {
            p.stats.insert(crew_stats[i as usize].clone(), 3);
        }

        if ctx.player {
            for skill in ctx.national_skills {
                if let Some(list) = defs::defs().skills.get(skill).and_then(|s| s.get("position_list")) {
                    if !defs::strings(list).contains(&position.name) {
                        continue;
                    }
                }
                p.skills.push(skill.clone());
            }
        }
        for skill in p.skills.clone() {
            match skill.as_str() {
                "Defend the Motherland" => *p.stats.get_mut("Morale").unwrap() += 3,
                "To Victory!" => *p.stats.get_mut("Grit").unwrap() += 3,
                _ => {}
            }
        }

        if ["Commander", "Commander/Gunner"].contains(&position.name.as_str()) {
            p.level = 4;
            p.exp = exp_required_for(p.level);
            p.adv = 4;
            p.age += rng.int(3, 9);
            p.rank = 2;
        } else if ["Gunner", "Gunner/Loader"].contains(&position.name.as_str()) {
            p.level = 2;
            p.exp = exp_required_for(p.level);
            p.adv = 2;
            p.age += rng.int(2, 5);
            p.rank = 1;
        }

        // the original picked the day from `monthrange()`, which is the
        // month's first weekday and its length, so a birthday fell on day
        // 0 to 6 or the last of the month; any day of it here (display)
        let year = ctx.today.split('.').next().and_then(|y| y.parse::<i32>().ok()).unwrap_or(1940) - p.age;
        let month = rng.int(1, 12);
        let day = rng.int(1, days_in_month(year, month));
        p.birthday = format!("{year}.{month:02}.{day:02}");

        p.set_ce(position);
        p
    }

    /// `GenerateName`: a first name and surname of the crewman's nation,
    /// not the same.
    pub fn generate_name(&mut self, rng: &mut Rng) {
        let nation = &defs::defs().nations[&self.nation];
        let first = defs::strings(&nation["first_names"]);
        let last = defs::strings(&nation["surnames"]);
        loop {
            let f = rng.choice(&first).clone();
            let l = rng.choice(&last).clone();
            if f != l {
                self.first_name = f;
                self.last_name = l;
                return;
            }
        }
    }

    /// The name as `DisplayName` shows it.
    pub fn name(&self, which: NameForm) -> String {
        match which {
            NameForm::Full => format!("{} {}", self.first_name, self.last_name),
            NameForm::First => self.first_name.clone(),
            NameForm::Last => self.last_name.clone(),
            NameForm::Initial => {
                format!("{}. {}", self.first_name.chars().next().unwrap_or(' '), self.last_name)
            }
        }
    }

    pub fn stat(&self, name: &str) -> i32 {
        self.stats.get(name).copied().unwrap_or(0)
    }

    /// `SetCEStatus`
    pub fn set_ce(&mut self, position: &Position) {
        self.ce = if !position.hatch {
            false
        } else if position.open_top || position.crew_always_ce {
            true
        } else {
            position.hatch_open
        };
    }

    /// `AwardExp`
    pub fn award_exp(&mut self, exp: i32) {
        self.exp += exp;
    }

    /// `PromotionCheck`, once a week: the message to show if promoted.
    pub fn promotion_check(&mut self, position: &str, rng: &mut Rng) -> Option<String> {
        if self.rank == 6 {
            return None;
        }
        let list = engine("LEVEL_RANK_LIST").as_object().expect("LEVEL_RANK_LIST");
        for (k, v) in list {
            if k.parse::<i32>().unwrap_or(99) <= self.level && int(v) > self.rank {
                if percentile(rng) <= num(engine("PROMOTION_CHANCE")) {
                    self.rank = int(v);
                    self.adv += 2;
                    let rank = rank_name(&self.nation, self.rank);
                    return Some(format!("Your {position} has been promoted to {rank}!"));
                }
                return None;
            }
        }
        None
    }

    /// `GetSkillMod`
    pub fn skill_mod(&self, modifier: f64) -> f64 {
        if !self.alive || ["Unconscious", "Critcal"].contains(&self.condition.as_str()) {
            return 0.0;
        }
        let mut m = modifier + self.stat("Knowledge") as f64 * 0.1 * modifier;
        if self.condition == "Stunned" {
            m *= 0.5;
        }
        if self.fatigue > 0 {
            m -= self.fatigue as f64;
        }
        let m = (m * 10.0).round_ties_even() / 10.0;
        m.max(0.0)
    }

    /// `DoFatigueCheck`
    pub fn fatigue_check(&mut self, rng: &mut Rng) {
        if !self.alive || ["Unconscious", "Critcal"].contains(&self.condition.as_str()) || self.fatigue == 10 {
            return;
        }
        if percentile(rng) <= 50.0 {
            return;
        }
        if percentile(rng) <= self.stat("Morale") as f64 * 10.0 {
            return;
        }
        self.fatigue += 1;
    }

    /// `Rest`
    pub fn rest(&mut self, rng: &mut Rng) {
        if !self.alive || self.fatigue == -5 {
            return;
        }
        self.fatigue = (self.fatigue - rng.int(0, self.stat("Morale"))).max(-5);
    }

    /// `ResolveInjuries`, at the end of a day: the recovery messages.
    pub fn resolve_injuries(&mut self, position: &str, rng: &mut Rng) -> Vec<String> {
        if !self.alive {
            return Vec::new();
        }
        let (mut min, mut max, mut chance) = (0, 0, 0.0);
        for (_, v) in &self.injury {
            match v.as_deref() {
                Some("Heavy") => {
                    chance += 3.0;
                    if min == 0 {
                        min = 3;
                    } else {
                        min += rng.int(0, 1);
                    }
                    if max < 7 {
                        max = 7;
                    } else {
                        max += rng.int(0, 2);
                    }
                }
                Some("Serious") => {
                    chance += 15.0;
                    if min < 7 {
                        min = 7;
                    } else {
                        min += rng.int(2, 4);
                    }
                    if max < 21 {
                        max = 21;
                    } else {
                        max += rng.int(2, 4);
                    }
                }
                _ => {}
            }
        }
        self.field_hospital = (percentile(rng) <= chance).then_some((min, max));
        let mut messages = Vec::new();
        for (k, v) in &mut self.injury {
            if let Some(level) = v.take() {
                if self.field_hospital.is_none() {
                    messages.push(format!("Your {position} recovers from his {level} {k} injury."));
                }
            }
        }
        messages
    }

    /// Unconscious or worse: cannot act.
    pub fn incapacitated(&self) -> bool {
        !self.alive || ["Unconscious", "Critical"].contains(&self.condition.as_str())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NameForm {
    Full,
    First,
    Last,
    Initial,
}

/// A rank's name in a nation's army.
pub fn rank_name(nation: &str, rank: i32) -> String {
    defs::defs().nations[nation]["rank_names"][rank.to_string()].as_str().unwrap_or("").to_string()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Position {
    pub name: String,
    /// "Turret", "Hull", or none.
    pub location: Option<String>,
    pub hatch: bool,
    pub hatch_group: Option<i32>,
    pub open_top: bool,
    pub crew_always_ce: bool,
    pub ce_visible: Vec<i32>,
    pub bu_visible: Vec<i32>,
    pub crewman: Option<Personnel>,
    pub hatch_open: bool,
    pub visible_hexes: Vec<(i32, i32)>,
}

impl Position {
    pub fn new(d: &Dict) -> Self {
        let dirs = |k: &str| d.get(k).and_then(|v| v.as_array()).map(|a| a.iter().map(int).collect()).unwrap_or_default();
        Position {
            name: text(&d["name"]).to_string(),
            location: d.get("location").map(|v| text(v).to_string()),
            hatch: d.contains_key("hatch"),
            hatch_group: d.get("hatch_group").map(int),
            open_top: d.contains_key("open_top"),
            crew_always_ce: d.contains_key("crew_always_exposed"),
            ce_visible: dirs("ce_visible"),
            bu_visible: dirs("bu_visible"),
            crewman: None,
            hatch_open: false,
            visible_hexes: Vec::new(),
        }
    }

    /// Set the CE status of the crewman here, if any.
    pub fn update_ce(&mut self) {
        if let Some(mut c) = self.crewman.take() {
            c.set_ce(self);
            self.crewman = Some(c);
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Weapon {
    pub stats: Dict,
    pub max_range: i32,
    pub ammo_type: Option<String>,
    /// For guns with ammunition: the general stores and the ready rack.
    pub ammo_stores: Option<BTreeMap<String, i32>>,
    pub ready_rack: BTreeMap<String, i32>,
    pub max_ammo: i32,
    pub max_plus_extra_ammo: i32,
    pub rr_size: i32,
    pub using_rr: bool,
    pub covered_hexes: Vec<(i32, i32)>,
    pub fired: bool,
    pub maintained_rof: bool,
    pub jammed: bool,
    pub broken: bool,
}

impl Weapon {
    pub fn new(stats: &Dict) -> Self {
        let mut stats = stats.clone();
        let s = |k: &str, stats: &Dict| defs::get(stats, k).map(|v| text(v).to_string());
        if !stats.contains_key("name") {
            let kind = s("type", &stats).unwrap_or_default();
            let name = if kind == "Gun" {
                let mut t = format!("{}mm", s("calibre", &stats).unwrap_or_default());
                if let Some(lr) = s("long_range", &stats) {
                    t += &format!("({lr})");
                }
                t
            } else if defs::strings(engine("MG_WEAPONS")).contains(&kind) && s("calibre", &stats).is_some() {
                format!("{}mm MG", s("calibre", &stats).unwrap())
            } else {
                kind
            };
            stats.insert("name".into(), name.into());
        }
        let kind = s("type", &stats).unwrap_or_default();
        let max_range = match stats.remove("max_range") {
            Some(v) => int(&v),
            None if ["Turret MG", "Co-ax MG"].contains(&kind.as_str()) => 2,
            None if ["Hull MG", "AA MG"].contains(&kind.as_str()) => 1,
            None => 3,
        };
        let mut w = Weapon {
            stats,
            max_range,
            ammo_type: None,
            ammo_stores: None,
            ready_rack: BTreeMap::new(),
            max_ammo: 0,
            max_plus_extra_ammo: 0,
            rr_size: 0,
            using_rr: false,
            covered_hexes: Vec::new(),
            fired: false,
            maintained_rof: false,
            jammed: false,
            broken: false,
        };
        if kind == "Gun" && w.stats.contains_key("ammo_type_list") {
            w.ammo_stores = Some(BTreeMap::new());
            w.max_ammo = int(&w.stats["max_ammo"]);
            w.max_plus_extra_ammo = w.max_ammo + (w.max_ammo as f64 * 0.15).floor() as i32;
            w.rr_size = w.stats.get("rr_size").map(int).unwrap_or(6);
            w.load_gun_ammo();
        }
        w
    }

    /// `GetStat` as a string.
    pub fn stat(&self, name: &str) -> Option<&str> {
        defs::get(&self.stats, name).and_then(|v| v.as_str())
    }

    pub fn name(&self) -> &str {
        self.stat("name").unwrap_or("")
    }

    pub fn ammo_type_list(&self) -> Vec<String> {
        self.stats.get("ammo_type_list").map(defs::strings).unwrap_or_default()
    }

    /// `LoadGunAmmo`: the default load, which `ClearGunAmmo` empties for
    /// the player before the first day. Lists other than these three
    /// leave everything empty and no type selected, as in the original.
    pub fn load_gun_ammo(&mut self) {
        if self.stat("type") != Some("Gun") {
            return;
        }
        let list = self.ammo_type_list();
        let stores = self.ammo_stores.get_or_insert_with(BTreeMap::new);
        for t in &list {
            stores.insert(t.clone(), 0);
            self.ready_rack.insert(t.clone(), 0);
        }
        let max = self.max_ammo;
        if list.len() == 1 {
            stores.insert(list[0].clone(), max);
            self.ammo_type = Some(list[0].clone());
        } else if list == ["HE", "AP", "Smoke"] {
            let he = (max as f64 * 0.6) as i32;
            let smoke = (max as f64 * 0.1) as i32;
            stores.insert("HE".into(), he);
            stores.insert("Smoke".into(), smoke);
            stores.insert("AP".into(), max - he - smoke);
            self.ammo_type = Some("AP".into());
        } else if list == ["HE", "AP"] {
            let he = (max as f64 * 0.7) as i32;
            stores.insert("HE".into(), he);
            stores.insert("AP".into(), max - he);
            self.ammo_type = Some("AP".into());
        }
    }

    /// `CycleAmmo`
    pub fn cycle_ammo(&mut self) -> bool {
        let list = self.ammo_type_list();
        if list.len() <= 1 {
            return false;
        }
        let i = self.ammo_type.as_ref().and_then(|t| list.iter().position(|x| x == t)).unwrap_or(0);
        self.ammo_type = Some(list[(i + 1) % list.len()].clone());
        true
    }

    /// The total in the general stores.
    pub fn total_stores(&self) -> i32 {
        self.ammo_stores.as_ref().map(|s| s.values().sum()).unwrap_or(0)
    }

    pub fn total_rr(&self) -> i32 {
        self.ready_rack.values().sum()
    }

    /// `ManageRR`: move shells of the current type between the stores and
    /// the ready rack (positive into the rack); false if it cannot.
    pub fn manage_rr(&mut self, add: i32) -> bool {
        let Some(t) = self.ammo_type.clone() else { return false };
        let total = self.total_stores();
        let rr = self.total_rr();
        let Some(stores) = self.ammo_stores.as_mut() else { return false };
        let in_stores = stores.get(&t).copied().unwrap_or(0);
        let in_rr = self.ready_rack.get(&t).copied().unwrap_or(0);
        if rr + add > self.rr_size
            || in_stores - add < 0
            || in_rr + add < 0
            || total - add > self.max_plus_extra_ammo + 1
        {
            return false;
        }
        stores.insert(t.clone(), in_stores - add);
        self.ready_rack.insert(t, in_rr + add);
        true
    }

    /// `GetEffectiveFP`
    pub fn effective_fp(&self) -> i32 {
        if self.stat("type") != Some("Gun") {
            return 1;
        }
        if let Some(fp) = defs::get(&self.stats, "fp") {
            return int(fp);
        }
        let calibre = self.stats.get("calibre").map(int).unwrap_or(0);
        for pair in engine("HE_FP_EFFECT").as_array().into_iter().flatten() {
            if int(&pair[0]) <= calibre {
                return int(&pair[1]);
            }
        }
        1
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Unit {
    pub unit_id: String,
    pub nick_name: String,
    pub unit_name: String,
    /// 0 the player's side, 1 the enemy's.
    pub owning_player: i32,
    pub nation: String,
    pub positions: Vec<Position>,
    pub stats: Dict,
    pub weapons: Vec<Weapon>,

    // scenario statuses (`ResetMe`)
    pub alive: bool,
    pub hx: i32,
    pub hy: i32,
    pub terrain: Option<String>,
    pub spotted: bool,
    pub smoke: i32,
    pub hull_down: Vec<i32>,
    pub moving: bool,
    pub immobilized: bool,
    pub bogged: bool,
    pub fired: bool,
    pub facing: Option<i32>,
    pub turret_facing: Option<i32>,
    pub pinned: bool,
    pub reduced: bool,
    pub routed: bool,
}

impl Unit {
    /// `Unit.__init__`; `None` for an unknown unit type.
    pub fn new(unit_id: &str, rng: &mut Rng) -> Option<Self> {
        let Some(serde_json::Value::Object(def)) = defs::defs().unit_types.get(unit_id) else {
            log::error!("could not find unit id: {unit_id}");
            return None;
        };
        let mut stats = def.clone();
        let positions = stats
            .get("crew_positions")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|p| p.as_object()).map(Position::new).collect())
            .unwrap_or_default();
        if let Some(chance) = stats.get("HVSS").map(num) {
            if percentile(rng) <= chance {
                stats.insert("HVSS".into(), true.into());
            } else {
                stats.remove("HVSS");
            }
        }
        let weapons = stats
            .get("weapon_list")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|w| w.as_object()).map(Weapon::new).collect())
            .unwrap_or_default();
        if stats.contains_key("weapon_list") {
            stats.insert("weapon_list".into(), serde_json::Value::Null);
        }
        Some(Unit {
            unit_id: unit_id.to_string(),
            nick_name: stats.get("nick_name").map(|v| text(v).to_string()).unwrap_or_default(),
            unit_name: String::new(),
            owning_player: 0,
            nation: String::new(),
            positions,
            stats,
            weapons,
            alive: true,
            hx: 0,
            hy: 0,
            terrain: None,
            spotted: false,
            smoke: 0,
            hull_down: Vec::new(),
            moving: false,
            immobilized: false,
            bogged: false,
            fired: false,
            facing: None,
            turret_facing: None,
            pinned: false,
            reduced: false,
            routed: false,
        })
    }

    /// `GetStat` as a string (`None` if absent, null or not a string).
    pub fn stat(&self, name: &str) -> Option<&str> {
        defs::get(&self.stats, name).and_then(|v| v.as_str())
    }

    /// Whether a stat is present at all (`GetStat(...) is not None`).
    pub fn has(&self, name: &str) -> bool {
        defs::get(&self.stats, name).is_some()
    }

    /// `GenerateNewPersonnel`: a crewman for each empty position.
    pub fn generate_new_personnel(&mut self, ctx: &CrewContext, rng: &mut Rng) {
        let nation = self.nation.clone();
        for p in &mut self.positions {
            if p.crewman.is_none() {
                p.crewman = Some(Personnel::new(&nation, p, ctx, rng));
            }
        }
    }

    /// `ClearGunAmmo`
    pub fn clear_gun_ammo(&mut self) {
        for w in &mut self.weapons {
            if let Some(stores) = &mut w.ammo_stores {
                for v in stores.values_mut() {
                    *v = 0;
                }
            }
        }
    }

    /// `GetPersonnelByPosition`
    pub fn crewman_in(&self, position: &str) -> Option<&Personnel> {
        self.positions.iter().find(|p| p.name == position).and_then(|p| p.crewman.as_ref())
    }

    /// `GetName`
    pub fn name(&self) -> &str {
        if self.owning_player == 1 && !self.spotted { "Unspotted Unit" } else { &self.unit_id }
    }

    /// The crewman in a player position (Commander or Commander/Gunner).
    pub fn commander(&self) -> Option<&Personnel> {
        let player = defs::strings(engine("PLAYER_POSITIONS"));
        self.positions.iter().filter(|p| player.contains(&p.name)).find_map(|p| p.crewman.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> CrewContext<'static> {
        CrewContext { player: true, national_skills: &[], today: "1939.09.01" }
    }

    #[test]
    fn experience_levels() {
        assert_eq!(exp_required_for(1), 0);
        assert_eq!(exp_required_for(2), 10);
        // 10 + ceil(10 * 2^1.1) = 10 + 22
        assert_eq!(exp_required_for(3), 32);
    }

    #[test]
    fn every_unit_type_builds() {
        let mut rng = Rng::new(5);
        for id in defs::defs().unit_types.keys() {
            let u = Unit::new(id, &mut rng).unwrap();
            assert_eq!(u.unit_id, *id);
        }
    }

    #[test]
    fn a_7tp_and_its_crew() {
        let mut rng = Rng::new(11);
        let mut u = Unit::new("7TP", &mut rng).unwrap();
        u.nation = "Poland".into();
        u.generate_new_personnel(&ctx(), &mut rng);
        assert!(u.positions.iter().all(|p| p.crewman.is_some()));
        let gun = &u.weapons[0];
        assert_eq!(gun.name(), "37mm(L)");
        assert!(gun.ammo_stores.is_some());
        let commander = u.crewman_in("Commander").unwrap();
        assert_eq!((commander.level, commander.rank), (4, 2));
        assert_eq!(commander.exp, exp_required_for(4));
        let threes = commander.stats.values().filter(|&&v| v == 3).count();
        assert_eq!(threes, 2);
        assert_eq!(commander.stat("Knowledge"), 1);
        let (_, month_day) = commander.birthday.split_at(5);
        let mut md = month_day.split('.');
        let (m, d): (i32, i32) = (md.next().unwrap().parse().unwrap(), md.next().unwrap().parse().unwrap());
        assert!((1..=12).contains(&m) && d >= 1 && d <= 31);
    }

    #[test]
    fn default_loads_fill_the_stores() {
        let mut rng = Rng::new(2);
        for id in defs::defs().unit_types.keys() {
            let u = Unit::new(id, &mut rng).unwrap();
            for w in &u.weapons {
                let list = w.ammo_type_list();
                if w.ammo_stores.is_some() && (list.len() == 1 || list == ["HE", "AP"] || list == ["HE", "AP", "Smoke"]) {
                    assert_eq!(w.total_stores(), w.max_ammo, "{id}");
                }
            }
        }
    }

    #[test]
    fn the_ready_rack() {
        let mut rng = Rng::new(2);
        let mut u = Unit::new("7TP", &mut rng).unwrap();
        let w = &mut u.weapons[0];
        let rr = w.rr_size;
        for _ in 0..rr {
            assert!(w.manage_rr(1));
        }
        assert!(!w.manage_rr(1));
        assert_eq!(w.total_rr(), rr);
    }
}
