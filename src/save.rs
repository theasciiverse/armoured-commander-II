//! Saved campaigns (`SaveGame`, `LoadGame`, `EraseGame` and
//! `LoadCampaignMenu` in armcom2.py). As in the original, each campaign
//! has its own folder in `saved_campaigns/` (in the player's data folder),
//! so there can be one saved game per campaign; Continue loads the most
//! recently saved. The original shelved the campaign, campaign day and
//! scenario objects; here they are JSON.

use crate::campaign::Campaign;
use crate::cday::CampaignDay;
use crate::color;
use crate::console::Align;
use crate::game::*;
use crate::layout::*;
use crate::ui::Vk;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Bumped whenever the saved state changes shape; saves of another format
/// are not listed, as the original leaves out saves of another major
/// version.
pub const FORMAT: u32 = 2;

#[derive(Serialize)]
struct SaveRef<'a> {
    format: u32,
    version: &'a str,
    /// Seconds since 1970, for "most recent".
    saved_at: u64,
    campaign: &'a Campaign,
    campaign_day: &'a Option<CampaignDay>,
    scenario: &'a Option<crate::battle::Scenario>,
}

#[derive(Deserialize)]
struct SaveFile {
    format: u32,
    #[allow(dead_code)]
    version: String,
    saved_at: u64,
    campaign: Campaign,
    campaign_day: Option<CampaignDay>,
    #[serde(default)]
    scenario: Option<crate::battle::Scenario>,
}

/// What the load menu shows of a save.
#[derive(Clone, Debug)]
pub struct SaveInfo {
    pub directory: String,
    pub saved_at: u64,
    pub campaign_name: String,
    pub total_vp: i32,
    pub date: String,
}

fn root() -> Option<PathBuf> {
    data_dir().map(|d| d.join("saved_campaigns"))
}

fn path(directory: &str) -> Option<PathBuf> {
    root().map(|r| r.join(directory).join("savegame.json"))
}

/// Whether a campaign has a saved game.
pub fn exists(directory: &str) -> bool {
    path(directory).is_some_and(|p| p.exists())
}

/// The saved game as JSON.
pub fn to_json(g: &Game) -> Result<String, String> {
    let camp = g.campaign.as_ref().ok_or("no campaign")?;
    let saved_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let save = SaveRef { format: FORMAT, version: VERSION, saved_at, campaign: camp, campaign_day: &g.campaign_day, scenario: &g.scenario };
    serde_json::to_string(&save).map_err(|e| e.to_string())
}

/// A saved game from JSON into `g`.
pub fn from_json(g: &mut Game, json: &str) -> Result<(), String> {
    let file: SaveFile = serde_json::from_str(json).map_err(|e| e.to_string())?;
    if file.format != FORMAT {
        return Err(format!("save format {} (this version reads {FORMAT})", file.format));
    }
    g.campaign = Some(file.campaign);
    g.campaign_day = file.campaign_day;
    g.scenario = file.scenario;
    Ok(())
}

/// `SaveGame`
pub fn save_game(g: &Game) {
    let Some(camp) = &g.campaign else { return };
    let Some(p) = path(&camp.filename) else { return };
    let result = (|| -> Result<(), String> {
        std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
        let json = to_json(g)?;
        // write beside it and rename, so a crash never leaves half a save
        let tmp = p.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &p).map_err(|e| e.to_string())
    })();
    match result {
        Ok(()) => log::info!("saved: {} {}", camp.filename, camp.today),
        Err(e) => log::error!("save {}: {e}", p.display()),
    }
}

/// `LoadGame`
pub fn load_game(g: &mut Game, directory: &str) -> bool {
    let Some(p) = path(directory) else { return false };
    match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|s| from_json(g, &s)) {
        Ok(()) => {
            log::info!("loaded: {directory} {}", g.camp().today);
            true
        }
        Err(e) => {
            log::error!("load {}: {e}", p.display());
            false
        }
    }
}

/// `EraseGame`
pub fn erase_game(directory: &str) {
    if let Some(p) = path(directory) {
        let _ = std::fs::remove_file(&p);
        if let Some(dir) = p.parent() {
            let _ = std::fs::remove_dir(dir);
        }
        log::info!("erased save: {directory}");
    }
}

/// The saves that can be loaded, most recent first.
pub fn list() -> Vec<SaveInfo> {
    let Some(root) = root() else { return Vec::new() };
    let Ok(dirs) = std::fs::read_dir(&root) else { return Vec::new() };
    let mut list = Vec::new();
    for d in dirs.flatten() {
        let directory = d.file_name().to_string_lossy().to_string();
        let Ok(text) = std::fs::read_to_string(d.path().join("savegame.json")) else { continue };
        let Ok(f) = serde_json::from_str::<SaveFile>(&text) else {
            log::warn!("save {directory} cannot be read; not listed");
            continue;
        };
        if f.format != FORMAT {
            continue;
        }
        list.push(SaveInfo {
            directory,
            saved_at: f.saved_at,
            campaign_name: crate::defs::text(&f.campaign.stats["name"]).to_string(),
            total_vp: f.campaign.player_vp,
            date: f.campaign.today.clone(),
        });
    }
    list.sort_by(|a, b| b.saved_at.cmp(&a.saved_at));
    list
}

impl Game {
    /// `LoadCampaignMenu`: the most recent save with `continue_most_recent`,
    /// else the player's choice. False if nothing was loaded.
    pub fn load_campaign_menu(&mut self, continue_most_recent: bool) -> bool {
        let saves = list();
        if saves.is_empty() {
            return false;
        }
        if continue_most_recent {
            return load_game(self, &saves[0].directory);
        }
        let mut selected = 0;
        self.draw_load_menu(&saves, selected);
        loop {
            self.flush();
            if self.take_redraw() {
                self.draw_load_menu(&saves, selected);
            }
            if !self.input() {
                continue;
            }
            match self.key().vk {
                Vk::Escape => return false,
                Vk::Enter => return load_game(self, &saves[selected].directory),
                _ => {}
            }
            match self.mapped_char() {
                'w' => selected = (selected + saves.len() - 1) % saves.len(),
                's' => selected = (selected + 1) % saves.len(),
                _ => continue,
            }
            self.draw_load_menu(&saves, selected);
        }
    }

    fn draw_load_menu(&mut self, saves: &[SaveInfo], selected: usize) {
        let keys = self.enkeys('w', 's');
        let c = &mut self.con;
        c.bg = color::BLACK;
        c.clear();
        c.fg = color::LIGHT_BLUE;
        c.print_ex(PAGE_W / 2, 1, Align::Center, "Load Saved Campaign");
        let mut y = (PAGE_H / 2 - saves.len() as i32).max(3);
        c.fg = color::WHITE;
        for (i, s) in saves.iter().enumerate() {
            if i == selected {
                c.bg = color::DARKER_BLUE;
                c.rect(2, y, 36, 1, true);
                c.bg = color::BLACK;
            }
            c.print_name(2, y, Align::Left, &s.campaign_name);
            y += 2;
        }
        let s = &saves[selected];
        let (x, y) = (74, 9);
        c.fg = color::WHITE;
        c.print_ex(x, y, Align::Center, "Saved Campaign");
        c.print_ex(x, y + 3, Align::Center, "Current VP");
        c.print_ex(x, y + 6, Align::Center, "Current Date");
        c.fg = color::LIGHT_GREY;
        c.print_name(x, y + 1, Align::Center, &s.campaign_name);
        c.print_ex(x, y + 4, Align::Center, &s.total_vp.to_string());
        c.print_ex(x, y + 7, Align::Center, &date_text(&s.date));
        key_line(c, 32, 38, 25, &keys, "Select Saved Campaign", color::WHITE);
        key_line(c, 32, 38, 26, "Enter", "Load and Continue Campaign", color::WHITE);
        key_line(c, 32, 38, 27, "Esc", "Return to Main Menu", color::WHITE);
        self.show_page();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cday::tests::new_game;

    #[test]
    fn a_save_in_a_battle_loads_back() {
        for seed in 1..6 {
            let mut g = new_game("pattons_best", seed);
            let zone = g.cd().player_unit_location;
            g.cdm().hexm(zone).controlled_by = 1;
            g.scenario = Some(crate::battle::Scenario::new(zone));
            g.setup_battle();
            for _ in 0..6 {
                g.scm().advance_phase = false;
                g.advance_to_next_phase();
                if g.scenario.as_ref().is_none_or(|s| s.finished) {
                    break;
                }
            }
            let json = to_json(&g).unwrap();
            let mut h = new_game("poland_campaign", 1);
            from_json(&mut h, &json).unwrap();
            assert_eq!(g.scenario.is_some(), h.scenario.is_some());
            let again = to_json(&h).unwrap();
            let strip = |s: &str| {
                let mut v: serde_json::Map<String, serde_json::Value> = serde_json::from_str(s).unwrap();
                v.remove("saved_at");
                v
            };
            assert_eq!(strip(&json), strip(&again));
        }
    }

    #[test]
    fn a_save_loads_back_the_same() {
        for (name, _) in &crate::defs::defs().campaigns {
            let g = new_game(name, 9);
            let json = to_json(&g).unwrap();
            let mut h = new_game("poland_campaign", 1);
            from_json(&mut h, &json).unwrap();
            let again = to_json(&h).unwrap();
            // the same but for the time saved
            let strip = |s: &str| {
                let v: serde_json::Value = serde_json::from_str(s).unwrap();
                let mut v = v.as_object().unwrap().clone();
                v.remove("saved_at");
                v
            };
            assert_eq!(strip(&json), strip(&again), "{name}");
        }
    }
}
