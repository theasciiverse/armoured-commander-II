//! The original's `CampaignDay` and `CDMapHex`: one combat day, its
//! mission, weather and map of 41 zones, and what happens around the day
//! (the ammunition load before it, crew replacement after).

use crate::campaign::{Records, new_records};
use crate::color::{self, Color};
use crate::console::Align;
use crate::defs::{self, Dict, engine, int, num, text};
use crate::game::*;
use crate::hexes;
use crate::layout::*;
use crate::model::{CrewContext, Personnel, Unit, percentile};
use crate::rng::Rng;
use crate::ui::Vk;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// `CAMPAIGN_DAY_HEXES`: the day map's zones, in the original's order.
pub fn cd_hexes() -> Vec<(i32, i32)> {
    engine("CAMPAIGN_DAY_HEXES")
        .as_array()
        .expect("CAMPAIGN_DAY_HEXES")
        .iter()
        .map(|p| (int(&p[0]), int(&p[1])))
        .collect()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CdMapHex {
    pub hx: i32,
    pub hy: i32,
    pub terrain_type: String,
    pub console_seed: i32,
    /// Road links in six directions: `Some(false)` dirt, `Some(true)` stone.
    pub road_links: [Option<bool>; 6],
    pub rivers: Vec<i32>,
    pub bridges: Vec<i32>,
    pub controlled_by: i32,
    pub known_to_player: bool,
    pub target_of_opportunity: Option<i32>,
    pub too_text: String,
    pub vp_value: i32,
    pub enemy_strength: i32,
    /// "A5" and so on. The original's formula has the same precedence
    /// slip as `GetCubeCoords`, so it is the row letter and 5 + hx: kept,
    /// as the labels are only shown.
    pub coordinate: String,
}

impl CdMapHex {
    pub fn new(hx: i32, hy: i32, mission: &str, week: &Dict, rng: &mut Rng) -> Self {
        let mut h = CdMapHex {
            hx,
            hy,
            terrain_type: String::new(),
            console_seed: rng.int(1, 128),
            road_links: [None; 6],
            rivers: Vec::new(),
            bridges: Vec::new(),
            controlled_by: 1,
            known_to_player: false,
            target_of_opportunity: None,
            too_text: String::new(),
            vp_value: 0,
            enemy_strength: 0,
            coordinate: format!("{}{}", (b'A' as i32 + hy) as u8 as char, 5 + hx),
        };
        h.set_enemy_strength(mission, week, rng);
        h
    }

    /// `SetEnemyStrength`
    pub fn set_enemy_strength(&mut self, mission: &str, week: &Dict, rng: &mut Rng) {
        self.known_to_player = false;
        let mut avg = week.get("average_resistance").map(int).unwrap_or(5);
        match mission {
            "Advance" => avg -= 1,
            "Battle" | "Fighting Withdrawal" => avg += 2,
            "Counterattack" => avg += 1,
            _ => {}
        }
        let s = (rng.int(0, avg) + rng.int(0, avg)) / 2;
        self.enemy_strength = s.clamp(1, 10);
    }

    /// `GenerateTerrainType`
    pub fn generate_terrain_type(&mut self, region: &Dict, week: &Dict, rng: &mut Rng) {
        let mut odds = region["cd_terrain_odds"].as_object().cloned().unwrap_or_default();
        if let Some(Value::Object(m)) = week.get("terrain_odds_modifier") {
            for (k, v) in m {
                odds.insert(k.clone(), v.clone());
            }
        }
        let total: i32 = odds.values().map(int).sum();
        let mut roll = rng.int(0, total);
        for (t, v) in &odds {
            if roll <= int(v) {
                self.terrain_type = t.clone();
                return;
            }
            roll -= int(v);
        }
    }

    pub fn has_road(&self) -> bool {
        self.road_links.iter().any(Option::is_some)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Weather {
    pub cloud_cover: String,
    pub precipitation: String,
    pub fog: i32,
    pub ground: String,
    pub freezing: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CampaignDay {
    pub records: Records,
    pub started: bool,
    pub ended: bool,
    pub maps_traversed: i32,
    pub mission: String,
    pub day_vp: i32,
    pub travel_time_spent: bool,
    pub weather: Weather,
    pub weather_update_clock: i32,
    pub fate_points: i32,
    pub player_squad: Vec<Unit>,
    /// (hour, minute)
    pub day_clock: (i32, i32),
    pub end_of_day: (i32, i32),
    /// Minutes.
    pub day_length: i32,
    pub random_event_chance: f64,
    /// Saved as a list: JSON keys are strings.
    #[serde(with = "zones")]
    pub map_hexes: BTreeMap<(i32, i32), CdMapHex>,
    pub player_unit_location: (i32, i32),
    pub active_menu: i32,
    pub selected_position: usize,
    pub selected_direction: Option<i32>,
    pub abandoned_tank: bool,
    pub player_withdrew: bool,
    /// Indexes of the player tank's guns.
    pub gun_list: Vec<usize>,
    pub selected_gun: Option<usize>,
    pub air_support_level: f64,
    pub arty_support_level: f64,
    pub advancing_fire: bool,
    pub air_support_request: bool,
    pub arty_support_request: bool,
    pub encounter_mod: f64,
}

impl CampaignDay {
    pub fn hex(&self, h: (i32, i32)) -> &CdMapHex {
        &self.map_hexes[&h]
    }

    pub fn hexm(&mut self, h: (i32, i32)) -> &mut CdMapHex {
        self.map_hexes.get_mut(&h).expect("a day map zone")
    }

    pub fn on_map(&self, h: (i32, i32)) -> bool {
        self.map_hexes.contains_key(&h)
    }

    /// `AddRecord`, the day's half (the campaign's is added by the caller).
    pub fn add_record(&mut self, name: &str, i: i32) {
        *self.records.entry(name.to_string()).or_insert(0) += i;
    }

    /// `GetHexPath` on the day map: the zones from one to the other, or
    /// empty if there is no way.
    pub fn hex_path(&self, from: (i32, i32), to: (i32, i32), rivers_block: bool, enemy_zones_block: bool) -> Vec<(i32, i32)> {
        #[derive(Clone, Copy)]
        struct Node {
            g: i32,
            f: i32,
            parent: Option<(i32, i32)>,
        }
        let h = |p: (i32, i32)| hexes::distance(p.0, p.1, to.0, to.1);
        let mut open: Vec<(i32, i32)> = vec![from];
        let mut closed: Vec<(i32, i32)> = Vec::new();
        let mut nodes: BTreeMap<(i32, i32), Node> = BTreeMap::new();
        nodes.insert(from, Node { g: 0, f: h(from), parent: None });
        while !open.is_empty() {
            let (i, &current) = open.iter().enumerate().min_by_key(|(_, p)| nodes[p].f).expect("open");
            if current == to {
                let mut path = vec![current];
                let mut n = nodes[&current];
                while let Some(p) = n.parent {
                    path.push(p);
                    n = nodes[&p];
                }
                path.reverse();
                return path;
            }
            open.remove(i);
            closed.push(current);
            for d in 0..6 {
                let next = hexes::adjacent_cd(current.0, current.1, d);
                let Some(node) = self.map_hexes.get(&next) else { continue };
                if closed.contains(&next) {
                    continue;
                }
                if rivers_block {
                    let here = self.hex(current);
                    if here.rivers.contains(&d) && !here.bridges.contains(&d) {
                        continue;
                    }
                    let back = hexes::constrain_dir(d + 3);
                    if node.rivers.contains(&back) && !node.bridges.contains(&back) {
                        continue;
                    }
                }
                if enemy_zones_block && node.controlled_by == 1 {
                    continue;
                }
                let g = nodes[&current].g + 1;
                if !open.contains(&next) {
                    nodes.insert(next, Node { g, f: g + h(next), parent: Some(current) });
                    open.push(next);
                } else if g < nodes[&next].g {
                    nodes.insert(next, Node { g, f: g + h(next), parent: Some(current) });
                }
            }
        }
        Vec::new()
    }

    /// The zones on the map's edge.
    fn edge_hexes(&self) -> Vec<(i32, i32)> {
        cd_hexes()
            .into_iter()
            .filter(|&(hx, hy)| (0..6).any(|d| !self.on_map(hexes::adjacent_cd(hx, hy, d))))
            .collect()
    }

    /// `GenerateRoads`
    pub fn generate_roads(&mut self, region: &Dict, rng: &mut Rng) {
        for h in self.map_hexes.values_mut() {
            h.road_links = [None; 6];
        }
        let stone = percentile(rng) <= num(&region["stone_road_odds"]);
        let dirt = percentile(rng) <= num(&region["dirt_road_odds"]);
        if !dirt && !stone {
            return;
        }
        let edges = self.edge_hexes();
        let (hx1, hy1) = *rng.choice(&edges);
        let hx2 = -hx1;
        let hy2 = match hy1 {
            y if y > 4 => y - (y - 4) * 2,
            4 => 4,
            y => y + (4 - y) * 2,
        };
        let (mut hx, mut hy) = (hx1, hy1);
        while hx != hx2 || hy != hy2 {
            let dist = hexes::distance(hx, hy, hx2, hy2);
            let choices: Vec<i32> = (0..6)
                .filter(|&d| {
                    let (px, py) = hexes::adjacent_cd(hx, hy, d);
                    self.on_map((px, py)) && hexes::distance(px, py, hx2, hy2) < dist
                })
                .collect();
            if choices.is_empty() {
                break;
            }
            let d = *rng.choice(&choices);
            let next = hexes::adjacent_cd(hx, hy, d);
            self.hexm((hx, hy)).road_links[d as usize] = Some(stone);
            self.hexm(next).road_links[hexes::constrain_dir(d + 3) as usize] = Some(stone);
            (hx, hy) = next;
        }

        // link the villages to the nearest road, with dirt roads
        let mut villages: Vec<(i32, i32)> = cd_hexes()
            .into_iter()
            .filter(|&h| self.hex(h).terrain_type == "Villages" && !self.hex(h).has_road())
            .collect();
        shuffle(&mut villages, rng);
        for (hx1, hy1) in villages {
            let mut links: Vec<(i32, (i32, i32))> = cd_hexes()
                .into_iter()
                .filter(|&h| h != (hx1, hy1) && self.hex(h).has_road())
                .map(|h| (hexes::distance(hx1, hy1, h.0, h.1), h))
                .collect();
            if links.is_empty() {
                continue;
            }
            // a stable sort, as Python's
            links.sort_by_key(|l| l.0);
            let (_, (hx2, hy2)) = links[0];
            let line = hexes::line(hx1, hy1, hx2, hy2);
            for w in line.windows(2) {
                if !self.on_map(w[0]) || !self.on_map(w[1]) {
                    break;
                }
                let Some(d) = hexes::direction_to_adjacent_cd(w[0].0, w[0].1, w[1].0, w[1].1) else { break };
                self.hexm(w[0]).road_links[d as usize] = Some(false);
                self.hexm(w[1]).road_links[hexes::constrain_dir(d + 3) as usize] = Some(false);
            }
        }
    }

    /// `GenerateRivers`: up to two rivers along zone edges, with bridges,
    /// tried again until every start zone can still reach the far side.
    pub fn generate_rivers(&mut self, region: &Dict, rng: &mut Rng) {
        let Some(odds) = region.get("river_odds").map(num) else { return };
        for h in self.map_hexes.values_mut() {
            h.rivers.clear();
            h.bridges.clear();
        }
        let mut rivers = 0;
        if percentile(rng) <= odds {
            rivers += 1;
            if percentile(rng) <= odds {
                rivers += 1;
            }
        }
        if rivers == 0 {
            return;
        }
        for _ in 0..300 {
            for _ in 0..rivers {
                let mut edges = self.edge_hexes();
                let (hx1, hy1) = *rng.choice(&edges);
                shuffle(&mut edges, rng);
                let mut end = *edges.last().expect("edge zones");
                for &(x, y) in &edges {
                    end = (x, y);
                    if hexes::distance(hx1, hy1, x, y) >= 5 {
                        break;
                    }
                }
                let (hx2, hy2) = end;
                let line = hexes::line(hx1, hy1, hx2, hy2);
                let off_map_dir = |cd: &CampaignDay, h: (i32, i32)| {
                    (0..6).find(|&d| !cd.on_map(hexes::adjacent_cd(h.0, h.1, d))).unwrap_or(5)
                };
                for (index, &(hx, hy)) in line.iter().enumerate() {
                    if !self.on_map((hx, hy)) {
                        continue;
                    }
                    if percentile(rng) <= 2.0 {
                        break;
                    }
                    let d1 = if (hx, hy) == (hx1, hy1) {
                        off_map_dir(self, (hx, hy))
                    } else {
                        let p = line[index - 1];
                        hexes::direction_to_adjacent_cd(hx, hy, p.0, p.1).unwrap_or(0)
                    };
                    let d2 = if (hx, hy) == (hx2, hy2) {
                        off_map_dir(self, (hx, hy))
                    } else {
                        let n = line[index + 1];
                        hexes::direction_to_adjacent_cd(hx, hy, n.0, n.1).unwrap_or(0)
                    };
                    let mut path1 = Vec::new();
                    for i in 0..6 {
                        path1.push(hexes::constrain_dir(d1 + i));
                        if hexes::constrain_dir(d1 + i) == d2 {
                            break;
                        }
                    }
                    let mut path2 = Vec::new();
                    for i in 0..6 {
                        path2.push(hexes::constrain_dir(d1 - i));
                        if hexes::constrain_dir(d1 - i) == d2 {
                            break;
                        }
                    }
                    let path = if path1.len() < path2.len() { path1 } else { path2 };
                    for &d in &path[1..] {
                        let h = self.hexm((hx, hy));
                        if h.rivers.contains(&d) {
                            continue;
                        }
                        h.rivers.push(d);
                        if h.road_links[d as usize].is_some() {
                            if !h.bridges.contains(&d) {
                                h.bridges.push(d);
                            }
                            continue;
                        }
                        if !h.bridges.contains(&d) && percentile(rng) <= 10.0 {
                            self.hexm((hx, hy)).bridges.push(d);
                        }
                    }
                }
            }
            let starts = [(-4, 8), (-3, 8), (-2, 8), (-1, 8), (0, 8)];
            let ends = [(4, 0), (3, 0), (2, 0), (1, 0), (0, 0)];
            if starts.iter().zip(ends).all(|(&a, b)| !self.hex_path(a, b, true, false).is_empty()) {
                return;
            }
            for h in self.map_hexes.values_mut() {
                h.rivers.clear();
                h.bridges.clear();
            }
        }
    }

    /// `CalcCaptureVP` for one zone; `defensive` is the player's
    /// Defensive Strategy national skill.
    pub fn calc_capture_vp(&mut self, h: (i32, i32), defensive: bool) {
        let (_, py) = self.player_unit_location;
        let (mission, traversed) = (self.mission.clone(), self.maps_traversed);
        let levels = int(engine("SPEARHEAD_HEXROW_LEVELS"));
        let z = self.hexm(h);
        if mission == "Spearhead" {
            z.vp_value = if py <= z.hy { 0 } else { ((9 - z.hy) + traversed * 8) / levels };
            return;
        }
        z.vp_value = if ["Forest", "Hills", "Villages"].contains(&z.terrain_type.as_str()) { 2 } else { 1 };
        if mission == "Advance" {
            z.vp_value += 1;
        }
        if defensive {
            z.vp_value -= 1;
        }
        z.vp_value = z.vp_value.max(0);
    }

    /// `GenerateWeather`
    fn generate_weather(&mut self, region: &Dict, today: &str, rng: &mut Rng) {
        let seasons = region["season_weather_odds"].as_object().expect("season_weather_odds");
        let date = &today[today.find('.').map_or(0, |i| i + 1)..];
        let season = seasons
            .iter()
            .find(|(_, v)| date <= text(&v["end_date"]))
            .map(|(k, _)| k.clone())
            .unwrap_or_else(|| "Winter".into());
        let odds = &seasons[&season];
        let roll_table = |table: &Value, rng: &mut Rng| -> String {
            let mut roll = percentile(rng);
            let mut result = String::new();
            for (k, v) in table.as_object().into_iter().flatten() {
                result = k.clone();
                if roll <= num(v) {
                    break;
                }
                roll -= num(v);
            }
            result
        };
        self.weather.ground = roll_table(&odds["ground_conditions"], rng);
        if ["Snow", "Deep Snow"].contains(&self.weather.ground.as_str()) {
            self.weather.freezing = true;
        } else if self.weather.ground == "Dry" && percentile(rng) <= num(&odds["freezing"]) {
            self.weather.freezing = true;
        }
        self.weather.cloud_cover = roll_table(&odds["cloud_cover"], rng);
        let mut roll = percentile(rng);
        self.weather.precipitation = "None".into();
        for (k, v) in odds["precipitation"].as_object().into_iter().flatten() {
            let chance = num(v);
            if !self.weather.freezing && ["Light Snow", "Snow", "Blizzard"].contains(&k.as_str()) {
                roll -= chance;
                continue;
            }
            if self.weather.freezing && ["Rain", "Heavy Rain"].contains(&k.as_str()) {
                roll -= chance;
                continue;
            }
            if roll <= chance {
                self.weather.precipitation = k.clone();
                break;
            }
            roll -= chance;
        }
        if self.weather.precipitation != "None" && self.weather.cloud_cover == "Clear" {
            self.weather.cloud_cover = rng.choice(&["Scattered", "Heavy", "Overcast"]).to_string();
        }
        self.weather_update_clock = int(engine("BASE_WEATHER_UPDATE_CLOCK")) + rng.int(1, 16);
    }
}

/// The day map's zones, saved as a list of zones (each knows its place).
mod zones {
    use super::CdMapHex;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::BTreeMap;

    pub fn serialize<S: Serializer>(m: &BTreeMap<(i32, i32), CdMapHex>, s: S) -> Result<S::Ok, S::Error> {
        m.values().collect::<Vec<_>>().serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<BTreeMap<(i32, i32), CdMapHex>, D::Error> {
        let v: Vec<CdMapHex> = Vec::deserialize(d)?;
        Ok(v.into_iter().map(|h| ((h.hx, h.hy), h)).collect())
    }
}

/// `random.shuffle`
pub fn shuffle<T>(v: &mut [T], rng: &mut Rng) {
    for i in (1..v.len()).rev() {
        let j = rng.int(0, i as i32) as usize;
        v.swap(i, j);
    }
}

fn parse_clock(s: &str) -> (i32, i32) {
    let mut p = s.split(':').map(|x| x.trim().parse::<i32>().unwrap_or(0));
    (p.next().unwrap_or(0), p.next().unwrap_or(0))
}

impl Game {
    /// A new campaign day, as the original makes one: `CampaignDay()`,
    /// then each zone's capture VP, roads and rivers.
    pub fn new_campaign_day(&mut self) {
        let mut rng = std::mem::replace(&mut self.rng, Rng::new(1));
        let camp = self.campaign.as_mut().expect("a campaign");
        let week = camp.week().clone();
        let region = engine("REGIONS")[text(&camp.stats["region"])].as_object().expect("region").clone();

        // `GenerateMission`
        let mut roll = percentile(&mut rng);
        let mut mission = String::new();
        let odds = week["mission_odds"].as_object().expect("mission_odds");
        for (k, v) in odds {
            if roll <= num(v) {
                mission = k.clone();
                break;
            }
            roll -= num(v);
        }
        if mission.is_empty() {
            // the original's fallback reads `self.current_week`, which a
            // campaign day does not have, so it would crash; the first one
            log::warn!("unable to set a mission for today, choosing default");
            mission = odds.keys().next().cloned().unwrap_or_else(|| "Advance".into());
        }

        let day_clock = parse_clock(text(&week["day_start"]));
        let end_of_day = parse_clock(text(&week["day_end"]));
        let (mut hours, mut minutes) = (end_of_day.0 - day_clock.0, end_of_day.1 - day_clock.1);
        if minutes < 0 {
            hours -= 1;
            minutes += 60;
        }

        let mut cd = CampaignDay {
            records: new_records(),
            started: false,
            ended: false,
            maps_traversed: 0,
            mission: mission.clone(),
            day_vp: 0,
            travel_time_spent: false,
            weather: Weather::default(),
            weather_update_clock: 0,
            fate_points: 0,
            player_squad: Vec::new(),
            day_clock,
            end_of_day,
            day_length: minutes + hours * 60,
            random_event_chance: num(engine("BASE_CD_RANDOM_EVENT_CHANCE")),
            map_hexes: BTreeMap::new(),
            player_unit_location: (-2, 8),
            active_menu: 3,
            selected_position: 0,
            selected_direction: None,
            abandoned_tank: false,
            player_withdrew: false,
            gun_list: Vec::new(),
            selected_gun: None,
            air_support_level: week.get("air_support_level").map(num).unwrap_or(0.0),
            arty_support_level: week.get("arty_support_level").map(num).unwrap_or(0.0),
            advancing_fire: false,
            air_support_request: false,
            arty_support_request: false,
            encounter_mod: 0.0,
        };
        cd.generate_weather(&region, &camp.today, &mut rng);
        cd.fate_points = if camp.options.fate_points { rng.int(1, 2) } else { 0 };

        camp.player_squad_max = match camp.pu().stat("class") {
            Some("Tankette") => 4,
            Some("Light Tank" | "Tank Destroyer") => 3,
            Some("Medium Tank" | "Assault Gun") => 2,
            Some("Heavy Tank") => 1,
            _ => camp.player_squad_max,
        };
        spawn_player_squad(camp, &mut cd, &mut rng);

        for (hx, hy) in cd_hexes() {
            let mut h = CdMapHex::new(hx, hy, &mission, &week, &mut rng);
            h.generate_terrain_type(&region, &week, &mut rng);
            cd.map_hexes.insert((hx, hy), h);
        }
        let all = cd_hexes();
        match mission.as_str() {
            "Fighting Withdrawal" => all.iter().for_each(|&h| cd.hexm(h).controlled_by = 0),
            "Counterattack" => {
                all.iter().for_each(|&h| cd.hexm(h).controlled_by = 0);
                for hx in 0..5 {
                    if cd.on_map((hx, 0)) {
                        cd.hexm((hx, 0)).controlled_by = 1;
                    }
                }
            }
            "Battle" => {
                all.iter().for_each(|&h| cd.hexm(h).controlled_by = 1);
                for hy in 6..9 {
                    let hx1 = -(hy as f64 / 2.0).floor() as i32;
                    for hx in hx1..hx1 + 5 {
                        if cd.on_map((hx, hy)) {
                            cd.hexm((hx, hy)).controlled_by = 0;
                        }
                    }
                }
            }
            _ => {}
        }
        cd.player_unit_location = match mission.as_str() {
            "Fighting Withdrawal" => (2, 0),
            "Battle" => (-1, 6),
            "Counterattack" => (2, 1),
            _ => (-2, 8),
        };
        build_player_gun_list(camp.pu(), &mut cd);
        let loc = cd.player_unit_location;
        cd.hexm(loc).controlled_by = 0;

        let defensive = camp.check_national_skill("Defensive Strategy");
        for h in cd_hexes() {
            cd.calc_capture_vp(h, defensive);
        }
        cd.generate_roads(&region, &mut rng);
        cd.generate_rivers(&region, &mut rng);
        log::info!(
            "campaign day: {} {mission}, weather {:?}, fate points {}",
            camp.today,
            cd.weather,
            cd.fate_points
        );
        self.campaign_day = Some(cd);
        self.rng = rng;
    }

    /// `AmmoReloadMenu`: load the guns before the day, on the 100x30 page.
    pub fn ammo_reload_menu(&mut self) {
        let guns: Vec<usize> = self
            .camp()
            .pu()
            .weapons
            .iter()
            .enumerate()
            .filter(|(_, w)| w.stat("type") == Some("Gun") && w.ammo_stores.is_some())
            .map(|(i, _)| i)
            .collect();
        if guns.is_empty() {
            return;
        }
        if self.scr.is_headless() && self.script.is_empty() {
            // tests: the default load in every gun
            for &g in &guns {
                default_load(&mut self.campm().pum().weapons[g]);
            }
            return;
        }
        let mut st = AmmoMenu { gun: 0, ammo_type: String::new(), add_num: 1, use_rr: false };
        st.ammo_type = self.camp().pu().weapons[guns[0]].ammo_type_list().first().cloned().unwrap_or_default();
        self.draw_ammo_menu(&guns, &st);
        loop {
            self.flush();
            if self.take_redraw() {
                self.draw_ammo_menu(&guns, &st);
            }
            if !self.input() {
                continue;
            }
            if self.key().vk == Vk::Enter {
                let empty = guns.iter().any(|&g| self.camp().pu().weapons[g].total_stores() == 0);
                if !empty
                    || self.show_notification("No ammo loaded in at least one gun! Are you sure you want to proceed?", true)
                {
                    break;
                }
                self.draw_ammo_menu(&guns, &st);
                continue;
            }
            let k = self.mapped_char();
            let wi = guns[st.gun];
            match k {
                'q' => {
                    if guns.len() == 1 {
                        continue;
                    }
                    st.gun = (st.gun + 1) % guns.len();
                    // the original keeps the selected type, which the next
                    // gun may not have: its first type then
                    let list = self.camp().pu().weapons[guns[st.gun]].ammo_type_list();
                    if !list.contains(&st.ammo_type) {
                        st.ammo_type = list.first().cloned().unwrap_or_default();
                    }
                }
                'c' => {
                    let list = self.camp().pu().weapons[wi].ammo_type_list();
                    let i = list.iter().position(|x| *x == st.ammo_type).unwrap_or(0);
                    st.ammo_type = list[(i + 1) % list.len()].clone();
                }
                'r' => st.use_rr = !st.use_rr,
                'z' => st.add_num = if st.add_num == 1 { 10 } else { 1 },
                'd' | 'a' | 'x' => {
                    let (add, t, use_rr) = (st.add_num, st.ammo_type.clone(), st.use_rr);
                    let w = &mut self.campm().pum().weapons[wi];
                    let (ammo_num, rr_num) = (w.total_stores(), w.total_rr());
                    match k {
                        'd' if use_rr => {
                            if rr_num + add > w.rr_size {
                                continue;
                            }
                            *w.ready_rack.entry(t).or_insert(0) += add;
                        }
                        'd' => {
                            if ammo_num + add > w.max_plus_extra_ammo {
                                continue;
                            }
                            *w.ammo_stores.as_mut().unwrap().entry(t).or_insert(0) += add;
                        }
                        'a' => {
                            let slot = if use_rr { w.ready_rack.get_mut(&t) } else { w.ammo_stores.as_mut().unwrap().get_mut(&t) };
                            match slot {
                                Some(n) if *n - add >= 0 => *n -= add,
                                _ => continue,
                            }
                        }
                        _ => {
                            default_load(w);
                            crate::sound::play("shell_move_10");
                            self.draw_ammo_menu(&guns, &st);
                            continue;
                        }
                    }
                    crate::sound::play(if add == 1 { "shell_move_1" } else { "shell_move_10" });
                }
                _ => continue,
            }
            self.draw_ammo_menu(&guns, &st);
        }
    }

    pub(crate) fn draw_ammo_menu(&mut self, guns: &[usize], st: &AmmoMenu) {
        let keys: Vec<String> = ['q', 'c', 'r', 'd', 'a', 'z', 'x'].iter().map(|&k| self.enkey(k).to_ascii_uppercase().to_string()).collect();
        let camp = self.campaign.as_ref().expect("a campaign");
        let u = camp.pu();
        let w = &u.weapons[guns[st.gun]];
        let c = &mut self.con;
        c.bg = color::BLACK;
        c.clear();
        let xm = PAGE_W / 2;

        c.bg = color::DARK_BLUE;
        c.rect(0, 0, PAGE_W, 3, true);
        c.fg = color::WHITE;
        c.print_ex(xm, 1, Align::Center, "Ammo Load");
        c.bg = color::BLACK;

        // the tank's portrait, at half size
        let (px, py) = (xm - 12, 4);
        c.bg = PORTRAIT_BG_COL;
        c.rect(px, py, 25, 4, true);
        if let Some(p) = u.stat("portrait").and_then(crate::xp::by_name) {
            let (pw, ph) = crate::art::fit(&p, 25, 4);
            crate::art::draw(&p, c, px, py, pw, ph);
        }
        c.fg = color::WHITE;
        if !u.unit_name.is_empty() {
            c.print_name(px, py, Align::Left, &u.unit_name);
        }
        c.bg = color::BLACK;

        // the guns
        for (n, &g) in guns.iter().enumerate() {
            let y = 9 + n as i32;
            if n == st.gun {
                c.bg = color::DARK_BLUE;
                c.rect(xm - 10, y, 20, 1, true);
                c.bg = color::BLACK;
            }
            c.fg = color::WHITE;
            c.print_ex(xm, y, Align::Center, u.weapons[g].name());
        }

        // what the gun's ammunition is for
        let (x, mut y) = (2, 12);
        c.fg = color::LIGHT_BLUE;
        c.print_ex(x, y, Align::Left, "Available Ammo Types");
        y += 2;
        for t in w.ammo_type_list() {
            let (title, lines): (&str, &[&str]) = match t.as_str() {
                "HE" => ("High Explosive (HE)", &["Used against guns,", "infantry, and unarmoured", "vehicles."]),
                "AP" => ("Armour Penetrating (AP)", &["Used against armoured", "targets."]),
                "Smoke" => ("Smoke", &["Used to blind targets."]),
                _ => continue,
            };
            c.fg = color::WHITE;
            c.print_ex(x, y, Align::Left, title);
            c.fg = color::LIGHT_GREY;
            for (i, l) in lines.iter().enumerate() {
                c.print_ex(x, y + 1 + i as i32, Align::Left, l);
            }
            y += lines.len() as i32 + 2;
        }

        // the stores and the ready rack, a shell to a cell
        let (sx, rx, y0) = (30, 53, 12);
        c.fg = color::WHITE;
        c.print_ex(sx, y0, Align::Left, "Stores");
        c.print_ex(rx, y0, Align::Left, "Ready Rack");
        c.bg = color::DARKER_YELLOW;
        if st.use_rr { c.rect(rx, y0, 10, 1, false) } else { c.rect(sx, y0, 6, 1, false) }
        c.bg = color::BLACK;
        let ammo_col = |t: &str| -> Color {
            engine("AMMO_TYPE_COLOUR").get(t).and_then(|v| v.as_array()).map_or(color::WHITE, |a| {
                Color(int(&a[0]) as u8, int(&a[1]) as u8, int(&a[2]) as u8)
            })
        };
        let types = defs::strings(engine("AMMO_TYPES"));
        let grid = |c: &mut crate::console::Console, x: i32, per_row: i32, counts: &BTreeMap<String, i32>, size: i32, extra_from: Option<i32>| {
            let mut n = 0;
            for t in &types {
                for _ in 0..counts.get(t).copied().unwrap_or(0) {
                    let bg = if extra_from.is_some_and(|m| n >= m) { color::DARK_RED } else { color::BLACK };
                    c.put_char_ex(x + n % per_row, y0 + 2 + n / per_row, 7, ammo_col(t), bg);
                    n += 1;
                }
            }
            c.fg = color::DARKER_GREY;
            while n < size {
                c.put_char(x + n % per_row, y0 + 2 + n / per_row, 9);
                n += 1;
            }
        };
        grid(c, sx, 20, w.ammo_stores.as_ref().unwrap(), w.max_ammo, Some(w.max_ammo));
        grid(c, rx, 10, &w.ready_rack, w.rr_size, None);

        // the numbers
        let (nx, mut ny) = (68, 12);
        c.fg = color::WHITE;
        c.print_ex(nx + 13, ny, Align::Right, "RR");
        ny += 1;
        let stores = w.ammo_stores.as_ref().unwrap();
        for t in &types {
            let Some(n) = stores.get(t) else { continue };
            if *t == st.ammo_type {
                c.bg = color::DARK_BLUE;
                c.rect(nx + 2, ny, 12, 1, true);
                c.bg = color::BLACK;
            }
            let under = c.char_bg(nx, ny);
            c.put_char_ex(nx, ny, 7, ammo_col(t), under);
            c.fg = color::WHITE;
            c.print_ex(nx + 2, ny, Align::Left, t);
            c.fg = color::LIGHT_GREY;
            c.print_ex(nx + 10, ny, Align::Right, &n.to_string());
            c.print_ex(nx + 13, ny, Align::Right, &w.ready_rack.get(t).copied().unwrap_or(0).to_string());
            ny += 1;
        }
        ny += 1;
        c.fg = color::WHITE;
        c.print_ex(nx + 2, ny, Align::Left, "Max");
        c.fg = color::LIGHT_GREY;
        c.print_ex(nx + 10, ny, Align::Right, &w.max_ammo.to_string());
        c.print_ex(nx + 13, ny, Align::Right, &w.rr_size.to_string());
        c.fg = color::LIGHT_RED;
        c.print_ex(nx + 2, ny + 1, Align::Left, "Extra");
        c.print_ex(nx + 10, ny + 1, Align::Right, &(w.max_plus_extra_ammo - w.max_ammo).to_string());
        c.fg = color::LIGHT_GREY;
        c.print_ex(nx + 13, ny + 1, Align::Left, "-");

        // the commands
        let (kx, ky) = (68, 20);
        let add = st.add_num;
        for (i, (k, label)) in [
            (&keys[0], "Cycle Selected Gun".to_string()),
            (&keys[1], "Cycle Selected Ammo Type".to_string()),
            (&keys[2], "Toggle Ready Rack".to_string()),
            (&keys[3], format!("Load {add}")),
            (&keys[4], format!("Unload {add}")),
            (&keys[5], "Toggle 1/10".to_string()),
            (&keys[6], "Default Load".to_string()),
        ]
        .into_iter()
        .enumerate()
        {
            key_line(c, kx, kx + 2, ky + i as i32, k, &label, color::WHITE);
        }
        key_line(c, kx, kx + 6, ky + 8, "Enter", "Accept and Continue", color::WHITE);
        self.show_page();
    }

    /// `DoCrewReplacementCheck`, for the player's unit: crewmen back to
    /// their own positions, new recruits for the empty ones.
    pub fn crew_replacement_check(&mut self) {
        let camp = self.camp();
        if !camp.pu().alive || camp.ended {
            return;
        }
        let mut messages = Vec::new();
        let skills = camp.national_skills();
        let today = camp.today.clone();
        let mut rng = std::mem::replace(&mut self.rng, Rng::new(1));
        let unit = self.campm().pum();
        // the original's second pass, for crewmen displaced by those
        // returning, read the crewman's own `.crewman` and crashed; each
        // goes back to his own position if it is free
        let mut holding: Vec<Personnel> = Vec::new();
        for i in 0..unit.positions.len() {
            let Some(c) = &unit.positions[i].crewman else { continue };
            if c.normal_position == unit.positions[i].name {
                continue;
            }
            let Some(j) = unit.positions.iter().position(|p| p.name == c.normal_position) else { continue };
            let c = unit.positions[i].crewman.take().unwrap();
            if let Some(other) = unit.positions[j].crewman.take() {
                holding.push(other);
            }
            messages.push(format!("Your {} returns to his position.", c.normal_position));
            unit.positions[j].crewman = Some(c);
            unit.positions[j].update_ce();
        }
        for c in holding {
            if let Some(j) = unit.positions.iter().position(|p| p.name == c.normal_position && p.crewman.is_none()) {
                messages.push(format!("Your {} returns to his position.", c.normal_position));
                unit.positions[j].crewman = Some(c);
                unit.positions[j].update_ce();
            } else if let Some(j) = unit.positions.iter().position(|p| p.crewman.is_none()) {
                unit.positions[j].crewman = Some(c);
                unit.positions[j].update_ce();
            }
        }
        let nation = unit.nation.clone();
        let ctx = CrewContext { player: true, national_skills: &skills, today: &today };
        for p in &mut unit.positions {
            if p.crewman.is_none() {
                p.crewman = Some(Personnel::new(&nation, p, &ctx, &mut rng));
                messages.push(format!("A new crewman joins your crew in the {} position.", p.name));
            }
        }
        self.rng = rng;
        for m in messages {
            self.show_message(&m, false, None);
        }
    }
}

impl Game {
    /// A campaign set up without its screens, as New Campaign would: the
    /// campaign's first day, its first tank with a fresh crew, and the day
    /// ready to start. For tests and `--battle`.
    pub fn quick_campaign(&mut self, campaign: &str) {
        let mut c = crate::campaign::Campaign::new();
        let (name, data) = defs::defs().campaigns.iter().find(|(n, _)| n == campaign).unwrap_or(&defs::defs().campaigns[0]);
        c.filename = name.clone();
        c.stats = data.as_object().cloned().unwrap_or_default();
        c.generate_combat_calendar(&mut self.rng);
        c.today = c.combat_calendar.first().cloned().unwrap_or_default();
        let id = defs::strings(&c.stats["player_unit_list"])[0].clone();
        let mut u = Unit::new(&id, &mut self.rng).expect("the campaign's first tank");
        u.nation = text(&c.stats["player_nation"]).to_string();
        let skills = c.national_skills();
        let today = c.today.clone();
        u.generate_new_personnel(&CrewContext { player: true, national_skills: &skills, today: &today }, &mut self.rng);
        u.clear_gun_ammo();
        c.player_unit = Some(u);
        self.campaign = Some(c);
        self.new_campaign_day();
    }
}

pub(crate) struct AmmoMenu {
    /// Index into the gun list.
    pub gun: usize,
    pub ammo_type: String,
    pub add_num: i32,
    pub use_rr: bool,
}

/// The ammunition menu's Default Load (X).
pub(crate) fn default_load(w: &mut crate::model::Weapon) {
    let max = w.max_ammo;
    let rr = w.rr_size;
    let stores = w.ammo_stores.as_mut().unwrap();
    for v in stores.values_mut() {
        *v = 0;
    }
    for v in w.ready_rack.values_mut() {
        *v = 0;
    }
    let (he, ap, smoke) = (stores.contains_key("HE"), stores.contains_key("AP"), stores.contains_key("Smoke"));
    if he && !ap {
        stores.insert("HE".into(), max);
        w.ready_rack.insert("HE".into(), rr);
    } else if ap && !he {
        stores.insert("AP".into(), max);
        w.ready_rack.insert("AP".into(), rr);
    } else if ap && he && smoke {
        let h = (max as f64 * 0.65) as i32;
        let s = (max as f64 * 0.10) as i32;
        stores.insert("HE".into(), h);
        stores.insert("Smoke".into(), s);
        stores.insert("AP".into(), max - h - s);
        let rh = (rr as f64 * 0.75) as i32;
        w.ready_rack.insert("HE".into(), rh);
        w.ready_rack.insert("AP".into(), rr - rh);
    } else {
        let h = (max as f64 * 0.75) as i32;
        stores.insert("HE".into(), h);
        stores.insert("AP".into(), max - h);
        let rh = (rr as f64 * 0.75) as i32;
        w.ready_rack.insert("HE".into(), rh);
        w.ready_rack.insert("AP".into(), rr - rh);
    }
}

/// `SpawnPlayerSquad`
pub(crate) fn spawn_player_squad(camp: &crate::campaign::Campaign, cd: &mut CampaignDay, rng: &mut Rng) {
    let pu = camp.pu();
    let n = camp.player_squad_max - cd.player_squad.len() as i32;
    for _ in 0..n.max(0) {
        let id = match camp.stats.get("player_squad_list").and_then(|l| l.get(&pu.unit_id)) {
            Some(list) => rng.choice(&defs::strings(list)).clone(),
            None => pu.unit_id.clone(),
        };
        let Some(mut u) = Unit::new(&id, rng) else { continue };
        u.nation = pu.nation.clone();
        u.battle.ai = Some(crate::ai::Ai::default());
        let ctx = CrewContext { player: false, national_skills: &[], today: &camp.today };
        u.generate_new_personnel(&ctx, rng);
        cd.player_squad.push(u);
    }
}

/// `BuildPlayerGunList`
pub(crate) fn build_player_gun_list(pu: &Unit, cd: &mut CampaignDay) {
    cd.gun_list = pu.weapons.iter().enumerate().filter(|(_, w)| w.stat("type") == Some("Gun")).map(|(i, _)| i).collect();
    cd.selected_gun = cd.gun_list.first().copied();
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::campaign::Campaign;
    use crate::ui::Screen;

    /// A campaign with its first day, as New Campaign makes it, headless.
    pub fn new_game(campaign: &str, seed: u64) -> Game {
        let mut g = Game::new(Screen::headless(), Rng::new(seed));
        g.quick_campaign(campaign);
        g
    }

    #[test]
    fn every_campaign_makes_a_day_that_can_be_crossed() {
        for (name, _) in &defs::defs().campaigns {
            for seed in 1..20 {
                let g = new_game(name, seed);
                let cd = g.cd();
                assert_eq!(cd.map_hexes.len(), 41);
                assert!(cd.map_hexes.values().all(|h| !h.terrain_type.is_empty()), "{name}");
                assert!(!cd.hex_path((-2, 8), (2, 0), false, false).is_empty());
                assert!(cd.day_length > 0);
                assert!(!cd.weather.ground.is_empty() && !cd.weather.cloud_cover.is_empty());
                // roads link both ways
                for h in cd.map_hexes.values() {
                    for d in 0..6 {
                        if let Some(stone) = h.road_links[d as usize] {
                            let n = hexes::adjacent_cd(h.hx, h.hy, d);
                            assert_eq!(cd.hex(n).road_links[hexes::constrain_dir(d + 3) as usize], Some(stone));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_default_load_fills_the_gun() {
        let mut g = new_game("poland_campaign", 3);
        let w = &mut g.campm().pum().weapons[0];
        default_load(w);
        assert_eq!(w.total_stores(), w.max_ammo);
        assert_eq!(w.total_rr(), w.rr_size);
    }
}
