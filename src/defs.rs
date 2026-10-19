//! The definitions: the original's JSON files (unit types, nations, skills,
//! crew commands, keyboards, the campaigns), and its engine constants
//! (`data/engine_defs.json`, written from armcom2.py by
//! `tools/defs2json.py`), all built into the binary.
//!
//! The original reads these as dictionaries, often walking them in order
//! to roll odds (mission odds, terrain and weather tables), so they stay
//! JSON values with the files' key order kept (serde_json's
//! `preserve_order`), with helpers for the original's ways of reading
//! them: most numbers are strings ("17.0", "180") in the files.

use serde_json::{Map, Value};
use std::sync::OnceLock;

pub type Dict = Map<String, Value>;

pub struct Defs {
    pub unit_types: Dict,
    pub nations: Dict,
    pub skills: Dict,
    pub crew_commands: Dict,
    pub keyboards: Dict,
    /// The constants at the top of armcom2.py.
    pub engine: Dict,
    /// (file name without `.json`, contents) for each campaign.
    pub campaigns: Vec<(String, Value)>,
}

/// The campaign files in the original `campaigns/`.
const CAMPAIGN_FILES: &[(&str, &str)] = &[
    ("battle_of_france_fra", include_str!("../data/campaigns/battle_of_france_fra.json")),
    ("battle_of_france_ger", include_str!("../data/campaigns/battle_of_france_ger.json")),
    ("battle_of_france_uk", include_str!("../data/campaigns/battle_of_france_uk.json")),
    ("canadas_best", include_str!("../data/campaigns/canadas_best.json")),
    ("liberation_campaign", include_str!("../data/campaigns/liberation_campaign.json")),
    ("pattons_best", include_str!("../data/campaigns/pattons_best.json")),
    ("poland_campaign", include_str!("../data/campaigns/poland_campaign.json")),
    ("september_campaign", include_str!("../data/campaigns/september_campaign.json")),
    ("winter_war_finland", include_str!("../data/campaigns/winter_war_finland.json")),
    ("winter_war_soviet", include_str!("../data/campaigns/winter_war_soviet.json")),
];

fn dict(text: &str, name: &str) -> Dict {
    match serde_json::from_str(text) {
        Ok(Value::Object(m)) => m,
        _ => panic!("{name} is not a JSON object"),
    }
}

pub fn defs() -> &'static Defs {
    static DEFS: OnceLock<Defs> = OnceLock::new();
    DEFS.get_or_init(|| Defs {
        unit_types: dict(include_str!("../data/unit_type_defs.json"), "unit_type_defs.json"),
        nations: dict(include_str!("../data/nation_defs.json"), "nation_defs.json"),
        skills: dict(include_str!("../data/skill_defs.json"), "skill_defs.json"),
        crew_commands: dict(include_str!("../data/crew_command_defs.json"), "crew_command_defs.json"),
        keyboards: dict(include_str!("../data/keyboard_mapping.json"), "keyboard_mapping.json"),
        engine: dict(include_str!("../data/engine_defs.json"), "engine_defs.json"),
        campaigns: CAMPAIGN_FILES
            .iter()
            .filter_map(|(name, text)| match serde_json::from_str(text) {
                Ok(v) => Some((name.to_string(), v)),
                Err(e) => {
                    log::error!("campaign {name}: {e}");
                    None
                }
            })
            .collect(),
    })
}

/// An engine constant.
pub fn engine(name: &str) -> &'static Value {
    defs().engine.get(name).unwrap_or_else(|| panic!("engine constant {name}"))
}

/// A number from the data: a JSON number, or a string holding one.
pub fn num(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        Value::String(s) => s.trim().parse().unwrap_or(0.0),
        Value::Bool(b) => *b as i32 as f64,
        _ => 0.0,
    }
}

/// `int(...)` of a value from the data.
pub fn int(v: &Value) -> i32 {
    match v {
        Value::String(s) => s.trim().parse::<i32>().unwrap_or_else(|_| num(v) as i32),
        _ => num(v) as i32,
    }
}

/// A string from the data, `""` if it is not one.
pub fn text(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

/// `dict.get(key)` that treats JSON `null` as absent, as the original's
/// `GetStat` treats a stat set to `None`.
pub fn get<'a>(d: &'a Dict, key: &str) -> Option<&'a Value> {
    d.get(key).filter(|v| !v.is_null())
}

/// A list of strings from the data.
pub fn strings(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().map(|x| text(x).to_string()).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_loads() {
        let d = defs();
        assert_eq!(d.unit_types.len(), 128);
        assert_eq!(d.nations.len(), 8);
        assert_eq!(d.skills.len(), 28);
        assert_eq!(d.crew_commands.len(), 17);
        assert_eq!(d.campaigns.len(), 10);
        assert_eq!(text(engine("VERSION")), "3.0.2");
    }

    #[test]
    fn order_is_kept() {
        // mission odds are rolled in the file's order
        let (_, poland) = defs().campaigns.iter().find(|(n, _)| n == "poland_campaign").unwrap();
        let odds = poland["calendar_weeks"][0]["mission_odds"].as_object().unwrap();
        assert_eq!(odds.keys().collect::<Vec<_>>(), ["Advance", "Battle", "Spearhead"]);
    }

    #[test]
    fn every_portrait_and_flag_is_there() {
        // the original ships two units without their portraits, and checks
        // for the file before drawing one
        let has = |n: &str| crate::xp::FILES.iter().any(|(f, _)| *f == n);
        for (id, u) in &defs().unit_types {
            if let Some(p) = u.get("portrait") {
                assert!(has(text(p)) || ["BA-20", "SU-12"].contains(&id.as_str()), "{id}: {p}");
            }
        }
        for (n, d) in &defs().nations {
            assert!(has(text(&d["flag_image"])), "{n}");
        }
    }
}
