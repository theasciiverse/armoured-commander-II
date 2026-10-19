// Much of the shared layer (console, colours, input) is ahead of the
// screens that will use it.
#![allow(dead_code)]

mod ai;
mod art;
mod battle;
mod battle_ui;
mod calendar;
mod campaign;
mod cday;
mod color;
mod console;
mod daymap;
mod defs;
mod game;
mod glyph;
mod hexes;
mod layout;
mod model;
mod rng;
mod save;
mod sound;
mod title;
mod ui;
mod xp;

use console::{Align, Console};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use rng::Rng;
use std::io;
use std::time::{Duration, Instant};
use title::{Choice, MainMenu};

const HELP: &str = "armcom2 [--art]

Armoured Commander II, the World War II tank commander roguelike by
Gregory Adam Scott, on its way to the terminal. Being ported: New
Campaign, the calendar, the crew, the day map, the battles, the saves and
the sound are ported.

  --art   browse the game's REXPaint art (portraits, flags, maps);
          left/right to change picture, f to switch between its own size
          and the screen's, q to quit

Needs a terminal of at least 100x30.
Session log: RUST_LOG=debug armcom2 2>armcom2.log";

const FRAME: Duration = Duration::from_millis(20);
const SNAPSHOT_EVERY: Duration = Duration::from_secs(1);

enum Mode {
    Splash { con: Box<Console>, since: Instant },
    Menu(Box<MainMenu>),
    Art { index: usize, fit: bool, con: Box<Console> },
}

impl Mode {
    fn name(&self) -> String {
        match self {
            Mode::Splash { .. } => "splash".into(),
            Mode::Menu(_) => "menu".into(),
            Mode::Art { index, .. } => format!("art {}", xp::FILES[*index].0),
        }
    }
}

fn main() -> io::Result<()> {
    env_logger::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{HELP}");
        return Ok(());
    }
    let mut rng = Rng::from_clock();
    fit_screen();
    let mode = if args.iter().any(|a| a == "--art") {
        art_mode(0, false)
    } else {
        Mode::Splash { con: Box::new(title::splash()), since: Instant::now() }
    };
    log::info!("start: {}", mode.name());

    let terminal = ratatui::init();
    let result = run(terminal, mode, &mut rng);
    ratatui::restore();
    log::info!("quit");
    result
}

/// The screen size from the terminal's (`layout` keeps it).
fn fit_screen() {
    if let Ok((w, h)) = ratatui::crossterm::terminal::size() {
        layout::set_screen_size(w as i32, h as i32);
    }
}

/// One picture of the art, at its own size or fitted to the screen.
fn art_mode(index: usize, fit: bool) -> Mode {
    let (name, data) = xp::FILES[index];
    let src = xp::load(data).unwrap_or_else(|e| {
        log::error!("{name}: {e}");
        Console::new(1, 1)
    });
    let mut con = Console::new(layout::sw(), layout::sh());
    con.bg = color::DARKER_GREY;
    con.clear();
    let (cols, rows) = if fit {
        art::fit(&src, layout::sw(), layout::sh() - 1)
    } else {
        art::native_size(&src)
    };
    let (x, y) = ((con.w - cols) / 2, (con.h - 1 - rows) / 2);
    art::draw(&src, &mut con, x, y, cols, rows);
    let caption = format!(
        "{name}  {}x{} cells, shown {cols}x{rows}{}  ({}/{})  left/right: picture  f: {}  q: quit",
        src.w,
        src.h,
        if fit { " (fitted)" } else { "" },
        index + 1,
        xp::FILES.len(),
        if fit { "own size" } else { "fit screen" }
    );
    con.bg = Color(0, 200, 255);
    con.fg = color::BLACK;
    con.rect(0, con.h - 1, con.w, 1, false);
    con.print_ex(0, con.h - 1, Align::Left, &caption);
    Mode::Art { index, fit, con: Box::new(con) }
}

use color::Color;

#[derive(Debug, Clone, Copy)]
enum Play {
    New,
    Continue,
    Load,
}

/// A campaign from the main menu until it ends or the player saves and
/// quits; the terminal is the campaign's while it runs.
fn play_campaign(terminal: ratatui::DefaultTerminal, what: Play, rng: &mut Rng) -> io::Result<ratatui::DefaultTerminal> {
    sound::theme_pause();
    // the mouse, for the zone info on the day map
    let _ = ratatui::crossterm::execute!(io::stdout(), event::EnableMouseCapture);
    let mut g = game::Game::new(ui::Screen::new(Some(terminal)), Rng::new(rng.int(1, i32::MAX) as u64));
    let go = match what {
        Play::New => g.start_new_campaign(),
        Play::Continue => g.load_campaign_menu(true),
        Play::Load => g.load_campaign_menu(false),
    };
    if go {
        g.do_campaign_calendar_loop();
    }
    let mut terminal = g.scr.into_terminal().expect("the campaign hands the terminal back");
    // back on the main menu, the theme from the start
    sound::theme_start();
    let _ = ratatui::crossterm::execute!(io::stdout(), event::DisableMouseCapture);
    terminal.clear()?;
    Ok(terminal)
}

fn run(mut terminal: ratatui::DefaultTerminal, mut mode: Mode, rng: &mut Rng) -> io::Result<()> {
    let mut snapshot = Instant::now();
    loop {
        match &mut mode {
            Mode::Menu(menu) => {
                menu.tick();
            }
            Mode::Splash { since, .. } if since.elapsed() >= title::SPLASH_FOR => {
                fit_screen();
                sound::configure(game::Settings::load().sounds_enabled, game::Settings::load().master_volume);
                sound::theme_start();
                mode = Mode::Menu(Box::new(MainMenu::new(rng)));
                log::info!("switch: menu");
            }
            _ => {}
        }
        terminal.draw(|frame| {
            let area = frame.area();
            let buf = frame.buffer_mut();
            let con: &Console = match &mode {
                Mode::Splash { con, .. } => con,
                Mode::Menu(menu) => &menu.con,
                Mode::Art { con, .. } => con,
            };
            ui::present(con, area, buf);
        })?;

        if snapshot.elapsed() >= SNAPSHOT_EVERY {
            let size = terminal.size()?;
            log::debug!("state: mode={} terminal={}x{}", mode.name(), size.width, size.height);
            snapshot = Instant::now();
        }

        if !event::poll(FRAME)? {
            continue;
        }
        let ev = event::read()?;
        log::debug!("input: {ev:?}");
        if let Event::Resize(..) = ev {
            fit_screen();
            mode = match mode {
                Mode::Menu(_) => Mode::Menu(Box::new(MainMenu::new(rng))),
                Mode::Art { index, fit, .. } => art_mode(index, fit),
                Mode::Splash { since, .. } => Mode::Splash { con: Box::new(title::splash()), since },
            };
            continue;
        }
        let Event::Key(key) = ev else { continue };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Ok(());
        }
        match &mut mode {
            Mode::Splash { .. } => {
                // any key skips the logo
                fit_screen();
                let settings = game::Settings::load();
                sound::configure(settings.sounds_enabled, settings.master_volume);
                sound::theme_start();
                mode = Mode::Menu(Box::new(MainMenu::new(rng)));
                log::info!("switch: menu");
            }
            Mode::Menu(menu) => {
                let c = match key.code {
                    KeyCode::Char(c) => c,
                    KeyCode::Esc => '\u{1b}',
                    _ => continue,
                };
                let choice = menu.key(c);
                let Some(choice) = choice else { continue };
                let what = match choice {
                    Choice::Quit => return Ok(()),
                    Choice::NewCampaign => Play::New,
                    Choice::Continue => Play::Continue,
                    Choice::Load => Play::Load,
                };
                log::info!("switch: campaign ({what:?})");
                terminal = play_campaign(terminal, what, rng)?;
                fit_screen();
                mode = Mode::Menu(Box::new(MainMenu::new(rng)));
                log::info!("switch: menu");
            }
            Mode::Art { index, fit, .. } => {
                let n = xp::FILES.len();
                let (next, fit) = match key.code {
                    KeyCode::Right | KeyCode::Char(' ') => ((*index + 1) % n, *fit),
                    KeyCode::Left => ((*index + n - 1) % n, *fit),
                    KeyCode::Char('f') => (*index, !*fit),
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    _ => continue,
                };
                mode = art_mode(next, fit);
                log::info!("switch: {}", mode.name());
            }
        }
    }
}
