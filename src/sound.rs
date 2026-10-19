//! Sound (`InitMixer`, `PlaySound`, `PlaySoundFor` and the main theme in
//! armcom2.py): the original's effects and theme tune, `sounds/*.ogg` (GPL
//! with the rest), built into the binary. The original mixed effects on 16
//! SDL channels and played the theme as SDL music. Here one audio thread
//! mixes whatever is playing, the theme looping among the effects until
//! stopped. As in Armoured Commander I, on Linux ALSA is loaded when the
//! first sound plays (`alsa.rs`), so the binary does not need libasound
//! to start; elsewhere rodio's own output (cpal) is used. Without an
//! audio device the game plays on silently; tests never open it.

use crate::defs::{self, engine};
use crate::model::{Unit, Weapon};
use crate::rng::Rng;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, OnceLock};

/// The effects, by the names `PlaySoundFor` asks for.
pub const SOUNDS: &[(&str, &[u8])] = &[
    ("37mm_firing_00", include_bytes!("../data/sounds/37mm_firing_00.ogg")),
    ("37mm_firing_01", include_bytes!("../data/sounds/37mm_firing_01.ogg")),
    ("37mm_firing_02", include_bytes!("../data/sounds/37mm_firing_02.ogg")),
    ("37mm_firing_03", include_bytes!("../data/sounds/37mm_firing_03.ogg")),
    ("37mm_he_explosion_00", include_bytes!("../data/sounds/37mm_he_explosion_00.ogg")),
    ("37mm_he_explosion_01", include_bytes!("../data/sounds/37mm_he_explosion_01.ogg")),
    ("add_skill", include_bytes!("../data/sounds/add_skill.ogg")),
    ("armour_save_00", include_bytes!("../data/sounds/armour_save_00.ogg")),
    ("armour_save_01", include_bytes!("../data/sounds/armour_save_01.ogg")),
    ("at_rifle_firing", include_bytes!("../data/sounds/at_rifle_firing.ogg")),
    ("command_select", include_bytes!("../data/sounds/command_select.ogg")),
    ("flamethrower", include_bytes!("../data/sounds/flamethrower.ogg")),
    ("grenades", include_bytes!("../data/sounds/grenades.ogg")),
    ("hatch", include_bytes!("../data/sounds/hatch.ogg")),
    ("hull_down_save", include_bytes!("../data/sounds/hull_down_save.ogg")),
    ("light_tank_moving_00", include_bytes!("../data/sounds/light_tank_moving_00.ogg")),
    ("light_tank_moving_01", include_bytes!("../data/sounds/light_tank_moving_01.ogg")),
    ("light_tank_moving_02", include_bytes!("../data/sounds/light_tank_moving_02.ogg")),
    ("menu_select", include_bytes!("../data/sounds/menu_select.ogg")),
    ("plane_incoming_00", include_bytes!("../data/sounds/plane_incoming_00.ogg")),
    ("ricochet", include_bytes!("../data/sounds/ricochet.ogg")),
    ("rifle_fire_00", include_bytes!("../data/sounds/rifle_fire_00.ogg")),
    ("rifle_fire_01", include_bytes!("../data/sounds/rifle_fire_01.ogg")),
    ("rifle_fire_02", include_bytes!("../data/sounds/rifle_fire_02.ogg")),
    ("rifle_fire_03", include_bytes!("../data/sounds/rifle_fire_03.ogg")),
    ("shell_move_1", include_bytes!("../data/sounds/shell_move_1.ogg")),
    ("shell_move_10", include_bytes!("../data/sounds/shell_move_10.ogg")),
    ("smoke", include_bytes!("../data/sounds/smoke.ogg")),
    ("sniper_hit", include_bytes!("../data/sounds/sniper_hit.ogg")),
    ("stuka_divebomb_00", include_bytes!("../data/sounds/stuka_divebomb_00.ogg")),
    ("vehicle_explosion_00", include_bytes!("../data/sounds/vehicle_explosion_00.ogg")),
    ("wheeled_moving_00", include_bytes!("../data/sounds/wheeled_moving_00.ogg")),
    ("wheeled_moving_01", include_bytes!("../data/sounds/wheeled_moving_01.ogg")),
    ("wheeled_moving_02", include_bytes!("../data/sounds/wheeled_moving_02.ogg")),
    ("zb_53_mg_00", include_bytes!("../data/sounds/zb_53_mg_00.ogg")),
];

const THEME: &[u8] = include_bytes!("../data/sounds/armcom2_theme.ogg");

static ENABLED: AtomicBool = AtomicBool::new(true);
static VOLUME: AtomicU32 = AtomicU32::new(10);

/// The settings' sound switch and master volume (1 to 10).
pub fn configure(enabled: bool, volume: i32) {
    ENABLED.store(enabled, Ordering::Relaxed);
    VOLUME.store(volume.clamp(1, 10) as u32, Ordering::Relaxed);
}

/// `SetMasterVolume`: effects at volume x 12 of SDL's 128, music x 6.
fn effects_volume() -> f32 {
    (VOLUME.load(Ordering::Relaxed) * 12) as f32 / 128.0
}

fn music_volume() -> f32 {
    (VOLUME.load(Ordering::Relaxed) * 6) as f32 / 128.0
}

/// What the audio thread is asked to play: an effect at a volume, or the
/// theme, looping until its flag is set.
pub enum Command {
    Effect(&'static [u8], f32),
    Theme(f32, Arc<AtomicBool>),
}

/// The source a command plays.
pub fn source_for(cmd: Command) -> Option<Box<dyn rodio::Source + Send>> {
    use rodio::Source;
    let data = match &cmd {
        Command::Effect(data, _) => *data,
        Command::Theme(..) => THEME,
    };
    let source = match rodio::Decoder::new(std::io::Cursor::new(data)) {
        Ok(s) => s,
        Err(e) => {
            log::error!("sound: cannot decode: {e}");
            return None;
        }
    };
    Some(match cmd {
        Command::Effect(_, v) => Box::new(source.amplify(v)),
        Command::Theme(v, stop) => Box::new(source.repeat_infinite().amplify(v).stoppable().periodic_access(
            std::time::Duration::from_millis(50),
            move |s| {
                if stop.load(Ordering::Relaxed) {
                    s.stop();
                }
            },
        )),
    })
}

/// The audio thread's queue; `None` once it is known there is no audio.
static QUEUE: OnceLock<Option<Sender<Command>>> = OnceLock::new();

fn queue() -> Option<&'static Sender<Command>> {
    QUEUE
        .get_or_init(|| {
            if cfg!(test) {
                return None;
            }
            let (tx, rx) = mpsc::channel::<Command>();
            let spawned = std::thread::Builder::new().name("audio".into()).spawn(move || output(rx));
            spawned.ok().map(|_| tx)
        })
        .as_ref()
}

/// The audio thread: plays each sound sent, mixed with those playing.
#[cfg(not(target_os = "linux"))]
fn output(rx: mpsc::Receiver<Command>) {
    let mut sink = match rodio::DeviceSinkBuilder::open_default_sink() {
        Ok(s) => s,
        Err(e) => {
            log::warn!("no audio device, playing without sound: {e}");
            return;
        }
    };
    // rodio would print to the terminal, over the game
    sink.log_on_drop(false);
    log::info!("sound: audio device open");
    for cmd in rx {
        if let Some(source) = source_for(cmd) {
            sink.mixer().add(source);
        }
    }
}

#[cfg(target_os = "linux")]
fn output(rx: mpsc::Receiver<Command>) {
    crate::alsa::play(rx);
}

fn send(cmd: Command) {
    if let Some(tx) = queue() {
        let _ = tx.send(cmd);
    }
}

/// `PlaySound`: start an effect, alongside any already playing.
pub fn play(name: &str) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let Some((_, data)) = SOUNDS.iter().find(|(n, _)| *n == name) else {
        log::warn!("sound: unknown {name}");
        return;
    };
    log::trace!("sound: {name}");
    send(Command::Effect(data, effects_volume()));
}

/// The playing theme's stop flag.
static THEME_STOP: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);

/// The main theme, from the start, looping (`Mix_PlayMusic`; the original
/// rewinds it when coming back to the main menu).
pub fn theme_start() {
    theme_stop();
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let stop = Arc::new(AtomicBool::new(false));
    if let Ok(mut s) = THEME_STOP.lock() {
        *s = Some(stop.clone());
    }
    send(Command::Theme(music_volume(), stop));
}

/// `Mix_PauseMusic`, when a campaign starts (it starts again from the top
/// after, as the original rewinds it).
pub fn theme_pause() {
    theme_stop();
}

/// The theme stops: sound switched off, or the game quitting.
pub fn theme_stop() {
    if let Some(stop) = THEME_STOP.lock().ok().and_then(|mut s| s.take()) {
        stop.store(true, Ordering::Relaxed);
    }
}

/// The game is quitting.
pub fn shutdown() {
    theme_stop();
}

/// `PlaySoundFor(weapon, 'fire')`
pub fn play_fire(w: &Weapon, rng: &mut Rng) {
    let t = w.stat("type").unwrap_or("");
    if t == "Gun" {
        if w.name() == "AT Rifle" {
            play("at_rifle_firing");
        } else {
            // "temp - used for all large guns for now", the original says
            play(&format!("37mm_firing_0{}", rng.int(0, 3)));
        }
    } else if defs::strings(engine("MG_WEAPONS")).iter().any(|m| m == t) {
        play("zb_53_mg_00");
    } else if w.name() == "Rifles" {
        play(&format!("rifle_fire_0{}", rng.int(0, 3)));
    } else if w.name() == "Grenades" {
        play("grenades");
    } else if w.name() == "Flame Thrower" {
        play("flamethrower");
    }
}

/// `PlaySoundFor(None, 'he_explosion')`
pub fn play_he_explosion(rng: &mut Rng) {
    play(&format!("37mm_he_explosion_0{}", rng.int(0, 1)));
}

/// `PlaySoundFor(unit, 'armour_save')`
pub fn play_armour_save(rng: &mut Rng) {
    play(&format!("armour_save_0{}", rng.int(0, 1)));
}

/// `PlaySoundFor(unit, 'movement')`
pub fn play_movement(u: &Unit, rng: &mut Rng) {
    if ["Wheeled", "Fast Wheeled"].contains(&u.stat("movement_class").unwrap_or("")) {
        play(&format!("wheeled_moving_0{}", rng.int(0, 2)));
    } else if ["Tankette", "Light Tank", "Medium Tank", "Heavy Tank", "Half-Tracked"].contains(&u.stat("class").unwrap_or("")) {
        play(&format!("light_tank_moving_0{}", rng.int(0, 2)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sound_decodes() {
        for (name, data) in SOUNDS.iter().chain([("theme", THEME)].iter()) {
            assert!(rodio::Decoder::new(std::io::Cursor::new(*data)).is_ok(), "{name}");
        }
    }

    #[test]
    fn every_sound_the_game_plays_is_there() {
        let names: Vec<&str> = SOUNDS.iter().map(|(n, _)| *n).collect();
        for file in ["calendar", "cday", "daymap", "battle", "battle_ui", "ai", "model", "main", "title"] {
            let text = std::fs::read_to_string(format!("src/{file}.rs")).unwrap();
            for part in text.split("sound::play(\"").skip(1) {
                let name = &part[..part.find('"').unwrap()];
                assert!(names.contains(&name), "{file}.rs plays {name}");
            }
        }
        for prefix in ["37mm_firing_0", "rifle_fire_0"] {
            for n in 0..4 {
                assert!(names.contains(&format!("{prefix}{n}").as_str()));
            }
        }
        for n in 0..3 {
            assert!(names.contains(&format!("wheeled_moving_0{n}").as_str()));
            assert!(names.contains(&format!("light_tank_moving_0{n}").as_str()));
        }
    }
}
