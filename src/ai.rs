//! The original's `AI` class: how enemy units and the player's squad act
//! in a battle (`DoActivation`, `DoBestAttack`).

use crate::battle::direction_toward;
use crate::defs::{self, engine, num};
use crate::game::Game;
use crate::hexes::{constrain_dir, direction_to_adjacent, distance, ring};
use crate::model::percentile;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Ai {
    /// The kind of action chosen this activation.
    pub disposition: String,
    /// Ordered back from the battle.
    pub recall: bool,
}

impl Game {
    fn ai(&self, i: usize) -> Ai {
        self.su(i).battle.ai.clone().unwrap_or_default()
    }

    fn set_disposition(&mut self, i: usize, d: &str) {
        if let Some(ai) = &mut self.sum(i).battle.ai {
            ai.disposition = d.to_string();
        }
    }

    /// `AI.DoActivation`
    pub fn ai_activation(&mut self, i: usize) {
        if !self.su(i).alive {
            return;
        }
        let pl = self.pl();
        if self.cd().ended && !self.ai(i).recall && self.su(i).owning_player == 1 && percentile(&mut self.rng) <= 25.0 {
            if let Some(ai) = &mut self.sum(i).battle.ai {
                ai.recall = true;
            }
        }
        if self.ai(i).recall && percentile(&mut self.rng) <= 10.0 {
            let text = format!("{} withdraws from the battlefield.", self.uname(i));
            let h = (self.su(i).hx, self.su(i).hy);
            self.bmsg(&text, Some(h));
            self.destroy_me(i, None, None, true);
            return;
        }
        let roll = percentile(&mut self.rng);
        let s = self.sc();
        let me = &s.units[i];
        let has_los_to_enemy = s.order.iter().any(|&u| s.units[u].alive && s.units[u].owning_player != me.owning_player && s.los(i, u));
        let crew_vulnerable = !self.vulnerable_crew(pl).is_empty();
        let current_range = distance(0, 0, me.hx, me.hy);
        let cat = me.category().to_string();
        let class = me.stat("class").unwrap_or("").to_string();

        let mut d = if me.battle.transport.is_some() {
            if current_range >= 2 { "Movement" } else { "Unload Passengers" }
        } else if cat == "Infantry" {
            if roll <= 25.0 {
                if crew_vulnerable && roll <= 20.0 { "Harass Player" } else { "Attack Player" }
            } else if roll <= 40.0 {
                "Combat"
            } else if roll <= 60.0 {
                if me.pinned { "Combat" } else { "Movement" }
            } else {
                "None"
            }
        } else if cat == "Gun" {
            if !me.battle.deployed {
                "None"
            } else if roll <= 10.0 {
                "Attack Player"
            } else if roll <= 65.0 {
                "Combat"
            } else {
                "None"
            }
        } else if class == "Armoured Train Car" {
            if me.unit_id == "Locomotive" || roll > 40.0 { "None" } else { "Combat" }
        } else if class == "Truck" {
            if roll <= 60.0 { "None" } else { "Movement" }
        } else if roll <= 15.0 {
            if crew_vulnerable && roll <= 10.0 { "Harass Player" } else { "Attack Player" }
        } else if roll <= 40.0 {
            "Combat"
        } else if roll <= 65.0 {
            "Movement"
        } else {
            "None"
        }
        .to_string();

        if current_range > 3 && d == "Combat" {
            d = "None".into();
        }
        let recall = self.ai(i).recall;
        if recall && cat != "Gun" && d != "Movement" && percentile(&mut self.rng) <= 80.0 {
            d = "Movement".into();
        }
        if self.sc().ambush {
            let r = percentile(&mut self.rng);
            d = if r <= 50.0 {
                "Combat"
            } else if r <= 65.0 {
                "Attack Player"
            } else if r <= 75.0 {
                "Harass Player"
            } else {
                "None"
            }
            .into();
        }
        if self.sc().squad.contains(&i) && d != "None" && d != "Combat" {
            d = "Combat".into();
        }
        if (d == "Attack Player" || d == "Harass Player") && !self.sc().los(i, pl) {
            d = "Combat".into();
        }
        if d == "Combat" && cat != "Gun" && !has_los_to_enemy {
            d = "Reposition".into();
        }
        let me = self.su(i).clone();
        let moving = d == "Movement" || d == "Reposition";
        if class == "MG Team" && moving && percentile(&mut self.rng) <= 80.0 {
            d = "Combat".into();
        }
        let moving = d == "Movement" || d == "Reposition";
        if me.battle.dug_in && moving && percentile(&mut self.rng) <= 80.0 {
            d = "Combat".into();
        }
        let moving = d == "Movement" || d == "Reposition";
        if me.battle.entrenched && moving && percentile(&mut self.rng) <= 97.0 {
            d = "Combat".into();
        }
        if me.battle.fortified && (d == "Movement" || d == "Reposition") {
            d = "Combat".into();
        }
        if me.routed {
            d = if percentile(&mut self.rng) <= 50.0 { "None" } else { "Movement" }.into();
        }
        if me.immobilized && (d == "Movement" || d == "Reposition") {
            d = "Combat".into();
        }
        self.set_disposition(i, &d);

        match d.as_str() {
            "Reposition" => {
                {
                    let u = self.sum(i);
                    u.moving = true;
                    u.battle.dug_in = false;
                    u.battle.entrenched = false;
                }
                self.clear_acquired_targets(i, false);
                if self.breakdown_check(i) {
                    return;
                }
                self.generate_terrain(i);
                self.generate_unit_los(i);
                self.check_for_hd(i, false);
                self.set_smoke_level(i);
            }
            "Movement" => {
                let (hx0, hy0) = (me.hx, me.hy);
                let mut list = ring(hx0, hy0, 1);
                let mut n = list.len();
                while n > 0 {
                    n -= 1;
                    let (hx, hy) = list[n];
                    if (hx, hy) == (0, 0) {
                        list.remove(n);
                        continue;
                    }
                    let dist = distance(0, 0, hx, hy);
                    let remove = if me.battle.transport.is_some() {
                        dist >= current_range
                    } else if recall || me.routed {
                        dist < current_range
                    } else if dist == 4 {
                        percentile(&mut self.rng) <= 75.0
                    } else if dist != current_range {
                        percentile(&mut self.rng) <= 40.0
                    } else {
                        false
                    };
                    if remove {
                        list.remove(n);
                    }
                }
                if list.is_empty() {
                    return;
                }
                let (hx, hy) = *self.rng.choice(&list);
                if !self.sc().on_map((hx, hy)) {
                    self.remove_from_play(i);
                    return;
                }
                if me.facing.is_some() {
                    let dir = direction_to_adjacent(hx0, hy0, hx, hy);
                    let u = self.sum(i);
                    u.facing = Some(dir);
                    if u.turret_facing.is_some() {
                        u.turret_facing = Some(dir);
                    }
                }
                {
                    let u = self.sum(i);
                    u.moving = true;
                    u.battle.dug_in = false;
                    u.battle.entrenched = false;
                }
                self.clear_acquired_targets(i, false);
                let b = &self.su(i).battle;
                let chance = b.forward_move_chance + b.forward_move_bonus;
                if percentile(&mut self.rng) <= chance {
                    if self.breakdown_check(i) {
                        return;
                    }
                    if cat == "Gun" {
                        let text = format!("{} crew has abandoned their gun.", me.unit_id);
                        self.bmsg(&text, Some((hx0, hy0)));
                        self.destroy_me(i, None, None, true);
                        return;
                    }
                    {
                        let u = self.sum(i);
                        u.battle.forward_move_chance = num(engine("BASE_FORWARD_MOVE_CHANCE"));
                        u.battle.forward_move_bonus = 0.0;
                    }
                    let s = self.scm();
                    s.stack_mut((hx0, hy0)).retain(|&x| x != i);
                    s.units[i].hx = hx;
                    s.units[i].hy = hy;
                    s.stack_mut((hx, hy)).push(i);
                    self.update_all_covered_hexes(i);
                } else {
                    self.sum(i).battle.forward_move_bonus += num(engine("BASE_MOVE_BONUS"));
                }
                self.generate_terrain(i);
                self.generate_unit_los(i);
                self.check_for_hd(i, false);
                self.set_smoke_level(i);
            }
            "Unload Passengers" => {
                if percentile(&mut self.rng) > 80.0 {
                    return;
                }
                let Some(t) = me.battle.transport.clone() else { return };
                let nation = self.sc().enemy_nation.clone().unwrap_or_default();
                let Some(u) = self.new_enemy_unit(&t, &nation) else { return };
                self.spawn_at(u, me.hx, me.hy);
                self.generate_unit_los(u);
                if me.spotted {
                    self.sum(u).spotted = true;
                    let text = format!("{} has unloaded a {}!", self.uname(i), self.uname(u));
                    self.bmsg(&text, Some((me.hx, me.hy)));
                }
                self.sum(i).battle.transport = None;
            }
            "Combat" | "Attack Player" | "Harass Player" => {
                if me.routed {
                    return;
                }
                let targets: Vec<usize> = if d != "Combat" {
                    vec![pl]
                } else {
                    let s = self.sc();
                    s.order
                        .iter()
                        .copied()
                        .filter(|&u| u != pl && s.units[u].owning_player != me.owning_player && distance(0, 0, s.units[u].hx, s.units[u].hy) <= 3)
                        .collect()
                };
                if targets.is_empty() {
                    if ["Infantry", "Gun"].contains(&cat.as_str()) && !me.battle.dug_in && self.attempt_dig_in(i) && (me.owning_player == 0 || me.spotted) {
                        let text = format!("{} is now dug in.", me.unit_id);
                        self.bmsg(&text, Some((me.hx, me.hy)));
                    }
                    return;
                }
                self.do_best_attack(i, &targets);
            }
            _ => {}
        }
    }

    /// `AI.DoBestAttack`
    fn do_best_attack(&mut self, i: usize, targets: &[usize]) -> bool {
        let disposition = self.ai(i).disposition;
        let mg = defs::strings(engine("MG_WEAPONS"));
        let mut attacks: Vec<(usize, usize, String)> = Vec::new();
        for &t in targets {
            for w in 0..self.su(i).weapons.len() {
                let (me, tu) = (self.su(i), self.su(t));
                let wp = &me.weapons[w];
                if distance(me.hx, me.hy, tu.hx, tu.hy) > wp.max_range {
                    continue;
                }
                let wt = wp.stat("type").unwrap_or("").to_string();
                if disposition == "Harass Player" && wt == "Gun" {
                    continue;
                }
                if wt == "Gun" {
                    for ammo in wp.ammo_type_list() {
                        self.sum(i).weapons[w].ammo_type = Some(ammo.clone());
                        if self.check_attack(i, w, t, true).is_empty() {
                            attacks.push((w, t, ammo));
                        }
                    }
                    continue;
                }
                if self.check_attack(i, w, t, true).is_empty() {
                    attacks.push((w, t, String::new()));
                }
            }
        }
        if attacks.is_empty() {
            return false;
        }
        let mut scored: Vec<(f64, usize, usize, String)> = Vec::new();
        for (w, t, ammo) in attacks {
            let (me, tu) = (self.su(i), self.su(t));
            let wp = &me.weapons[w];
            let wt = wp.stat("type").unwrap_or("").to_string();
            let is_mg = mg.contains(&wt);
            if disposition != "Harass Player" && tu.category() == "Vehicle" && wt != "Gun" && wt != "Close Combat" && !is_mg {
                continue;
            }
            let (mut pivot, mut rotate) = (false, false);
            if let Some(mount) = wp.stat("mount") {
                let covered = wp.covered_hexes.contains(&(tu.hx, tu.hy));
                if mount == "Turret" && me.turret_facing.is_some() {
                    rotate = !covered;
                } else {
                    pivot = !covered;
                }
            }
            if pivot && self.sc().squad.contains(&i) {
                continue;
            }
            if !ammo.is_empty() {
                self.sum(i).weapons[w].ammo_type = Some(ammo.clone());
            }
            let Some(p) = self.calc_attack(i, w, t, pivot, rotate) else { continue };
            let (me, tu) = (self.su(i), self.su(t));
            let mut score = p.final_chance;
            if tu.category() == "Infantry" && ammo == "HE" {
                score -= 20.0;
            }
            if disposition != "Harass Player" && wt != "Gun" && wt != "Close Combat" && tu.has("armour") {
                if !is_mg || distance(me.hx, me.hy, tu.hx, tu.hy) > num(engine("MG_AP_RANGE")) as i32 {
                    score -= 40.0;
                }
            }
            if tu.category() == "Vehicle" && wt == "Gun" {
                if tu.has("armour") && ammo == "HE" {
                    score -= 20.0;
                }
                if !tu.has("armour") && ammo == "AP" {
                    score -= 20.0;
                }
            }
            if wt == "Close Combat" {
                score -= 15.0;
                if me.battle.fortified {
                    score -= 45.0;
                } else if me.battle.entrenched {
                    score -= 30.0;
                } else if me.battle.dug_in {
                    score -= 20.0;
                }
            }
            scored.push(((score * 100.0).round_ties_even() / 100.0, w, t, ammo));
        }
        if scored.is_empty() {
            return false;
        }
        // a stable sort, highest first, as Python's
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let (score, w0, t, ammo) = scored[0].clone();
        if score <= 3.0 {
            return false;
        }
        // the original sets the chosen ammunition on a misspelt attribute
        // (`current_ammo`), so the gun fires whatever type it was last set
        // to while scoring (kept)
        let mut w = w0;
        let (tx, ty) = (self.su(t).hx, self.su(t).hy);
        let mount = self.su(i).weapons[w0].stat("mount").map(str::to_string);
        if mount.is_some() && !self.su(i).weapons[w0].covered_hexes.contains(&(tx, ty)) {
            let me = self.su(i);
            let dir = direction_toward(me.hx, me.hy, tx, ty);
            if mount.as_deref() == Some("Turret") && me.turret_facing.is_some() {
                self.sum(i).turret_facing = Some(dir);
            } else if mount.as_deref() == Some("Hull") && me.facing.is_some() {
                let u = self.sum(i);
                u.facing = Some(dir);
                if u.turret_facing.is_some() {
                    u.turret_facing = Some(dir);
                }
                self.clear_acquired_targets(i, true);
            }
            self.update_all_covered_hexes(i);
            // the original updates the covered hexes in a loop that reuses
            // the name of the chosen weapon, so after turning, the unit
            // fires its last weapon instead (kept)
            w = self.su(i).weapons.len() - 1;
        }
        if mount.as_deref() == Some("Turret") {
            if let Some(blocked) = defs::get(&self.su(i).weapons[w].stats, "blocked_hull_dirs").map(defs::strings) {
                let me = self.su(i);
                let dir = constrain_dir(me.turret_facing.unwrap_or(0) - me.facing.unwrap_or(0));
                if blocked.contains(&dir.to_string()) {
                    let f = me.facing.unwrap_or(0);
                    self.sum(i).facing = Some(constrain_dir(f + 3));
                    self.clear_acquired_targets(i, true);
                    self.update_all_covered_hexes(i);
                    w = self.su(i).weapons.len() - 1;
                }
            }
        }
        self.move_to_top_of_stack(t);
        let me = self.su(i);
        if me.owning_player == 0 || me.spotted {
            let mut text = format!("{} attacks {} with {}", self.uname(i), self.uname(t), me.weapons[w].name());
            if !ammo.is_empty() {
                text += &format!(" ({ammo})");
            }
            text.push('.');
            self.scm().context_text = text;
        }
        self.draw_battle();
        self.flush();
        self.wait(20, false);
        let result = self.attack(i, w, Some(t));
        self.scm().context_text.clear();
        result
    }
}
