# Armoured Commander II

A Rust + Ratatui port of Armoured Commander II, Gregory Adam Scott's World
War II tank commander roguelike (2016 to 2020), for the Asciiverse
catalog. Like the first game it is made of character cells, drawn into
its own SDL window through libtcod; the port keeps the game and puts the
cells in the terminal.

Status: started 2026-10-19, after Armoured Commander I was finished
(`../armoured-commander`). The shared layer is taken over from I (console,
colours, dice, terminal); II's glyph table, its art built in and browsable
with `--art`, and the main menu (studio logo, title with a random tank,
the sweeping light, the options) work. 2026-10-19, second session: the
definitions, the crew, tank and weapon model, New Campaign (campaign and
tank selection), the calendar with its four tabs (the day's mission, crew
and tank, journal, field hospital), the crewman report and skills, swap
positions, nicknames and tank names, the ammunition load, the start and
end of each day (injuries, levels, the field hospital, crew replacement,
returning crewmen, a new tank on refits), the end of the campaign with its
summary and log, and saves (Save and Quit, Continue, Load). The same day,
third session: the campaign day map (travel, recon, support requests,
advancing fire, waiting, resupply, the crew and ready rack, abandoning
the tank, weather changes, random events, zones won and lost, new map
regions, the day's end and summary). Fourth session (same day): the battle
(`Scenario`, the AI, attacks, armour, firepower, morale, crew injuries,
the bail-out, air and artillery support, random events), so a campaign
plays from start to end. Then sound and the theme tune.
Not yet: release builds.

## Source and licence

- Original: github.com/sudasana/armcom2, last commit `9d719b6`
  (2020-05-03, version 3.0.2, "Bugfixes for 3.0.2"), cloned at
  `../../references/Armoured Commander II`. Python 3.6 and libtcod 1.6.4,
  one file, `armcom2.py`, 18,300 lines; definitions in JSON (`data/*.json`,
  ten campaigns in `campaigns/`). The `3.0.x` branch is an older point on
  the same line.
- GPL-3.0-or-later for the game and its data (`docs/license.txt` in the
  original, our `LICENSE`). The newer Steam versions are not GPL and are
  not used. `xp_loader.py` was MIT; `src/xp.rs` is our own reader, taken
  over from I.
- The art (`data/*.xp`), the definitions (`data/*.json`) and the campaigns
  (`data/campaigns/`) are the original's files, unchanged.
- Sounds: the original's `sounds/*.ogg` and its theme tune (GPL with
  the rest), unchanged in `data/sounds/`, built into the binary.
- Steamworks is dropped.
- The name and Gregory Adam Scott's blessing: as for I (David,
  2026-10-18), to be asked after the Asciiverse launch.

## How the port is built

As in I: the original draws with libtcod console functions, and
`src/console.rs` is that console model, so its drawing code translates
line for line. Taken over from I unchanged unless said: `console.rs`
(glyph codes now 16-bit, for II's 288 glyphs), `color.rs`, `rng.rs`,
`ui.rs` (the terminal: `Screen`, `present`, `wrap`), `xp.rs`.

- `src/glyph.rs`: code to Unicode for II's font, `c64_8x8_ext.png` (the
  same at 16x16 for the large font option): 16 glyphs a row, 18 rows. 0
  to 255 are CP437 except where the font differs, checked glyph by glyph
  against its bitmaps: 16, 17, 30, 31 corner triangles, 28, 29 corner
  brackets, 178 a two-quarter checker, 224 to 227 quarter blocks, 228 to
  231 diagonal arrows, 236 to 239 rounded corners (shown as triangles),
  249 a small square, 254 a vertical bar. 256 to 273 are the Polish
  letters `DisplayName` maps crew names onto. Two encoders: `encode` for
  the game's own strings (a character below 256 is its own code, as
  libtcod takes `chr(196)`), `encode_name` for names from the data (each
  letter looked up; the original went through IBM850, which the font does
  not follow, so some names came out wrong).
- `src/glyph_table.rs`, written by `tools/glyphs.py` from the font: each
  glyph's 2x2 pixel pattern. Rerun it rather than editing the table.
- `src/art.rs`: II's art is pixel art in square cells (blocks, half and
  quarter blocks, corner triangles: a 2x2 grid of pixels a cell). A half
  block `▀` in a terminal cell is two square pixels, so one original cell
  is two terminal columns of one row at the art's own size, and art can be
  drawn at any size by resampling its pixels. Glyphs that are drawings
  (letters, symbols, the poppy) are put in as characters, one to a column
  in runs, wherever there is a column or more per art cell. At 100x30 the
  title is half its own size, so two original rows share a terminal row
  and some lines of drawn text drop out (the cat's ears on the studio
  logo, part of the poppy's caption).
- `src/layout.rs`: the screen is the terminal's size, at least 100x30, as
  in I.
- `src/title.rs`: the studio logo with the disclaimer (2 seconds, or a
  key), then the main menu: `main_title.xp` with a random tank portrait
  and the poppy, drawn as big as fits in its proportions (90x30 at
  100x30), the menu and copyright as terminal text at the original's rows
  scaled, and the light sweeping across the title letters
  (`AnimateMainMenu`). Options open in place of the menu, as in the
  original. Continue and Load are greyed without a saved campaign.
- `src/defs.rs`: the JSON definitions, built in, read with serde_json's
  `preserve_order`, because the original rolls odds by walking its
  dictionaries in their file order (mission odds, terrain, weather).
  `data/engine_defs.json` holds the constants from the top of armcom2.py,
  written by `tools/defs2json.py` (which runs that part of the original
  against a stub libtcod); rerun it rather than editing the JSON.
- `src/model.rs`: `Personnel`, `Position`, `Weapon`, `Unit`. A unit owns
  its positions and each position owns its crewman (the original links
  them both ways); rules that would show a message return it for the
  caller to show.
- `src/game.rs`: the original's globals as one `Game`, and its general
  functions (messages, notifications, text input, the game menu and
  options, keyboard layouts with `DeKey` and `EnKey`, settings).
- `src/sound.rs`: `PlaySound`, `PlaySoundFor` and the theme. One audio
  thread mixes whatever is playing (rodio decodes and mixes), the theme
  looping among the effects until its stop flag is set. The volumes are
  the original's (effects master x 12, music x 6, of SDL's 128); the
  options' sound switch stops the theme on the main menu. The theme plays
  on the main menu, stops for a campaign and starts again from the top
  after it, as the original rewinds it. As in I, on Linux the output is
  `src/alsa.rs` (taken over from I): libasound is loaded with `dlopen`
  when the first sound plays, so the binary links nothing beyond libc and
  starts without ALSA, and ALSA's default device reaches PipeWire and
  PulseAudio (checked: a "PipeWire ALSA [armcom2]" stream). So Linux
  releases are glibc, not static musl. macOS and Windows use rodio's cpal
  output. Without an audio device the game plays silently; tests never
  open it.
- `src/battle.rs` (the battle's rules: the original's `Scenario`, and
  the battle half of `Unit`, `Weapon` and `Personnel`), `src/ai.rs`
  (`AI`), `src/battle_ui.rs` (the battle screen and its loop). A battle
  keeps its units in one list that only grows and refers to them by
  index; `order` keeps the original's `scenario.units` list, units
  leaving it as they leave play, and loops over it the way Python's do.
  The player's tank and squad are copied in at the start and back at the
  end, as the original does.
- `src/campaign.rs` (the campaign, its calendar, the selection screens,
  `DisplayMyInfo`), `src/cday.rs` (`CampaignDay` and `CDMapHex`: making a
  day, roads, rivers, the day map's paths, the ammunition load, crew
  replacement), `src/calendar.rs` (the calendar screen and loop, the crew
  screens, start and end of day, new tank, the campaign's end),
  `src/hexes.rs` (hex geometry), `src/save.rs` (saves).
- `src/save.rs`: as in the original, one saved game per campaign, each in
  its own folder (`saved_campaigns/<campaign>/savegame.json` in the data
  folder, `~/.local/share/armoured-commander-ii`); Continue loads the most
  recently saved, Load lists them. Saved at the original's points. The
  settings are `settings.json` beside them (the original's `armcom2.cfg`;
  its font size option is gone, as the terminal's font is the terminal's).
  `save::FORMAT` is bumped whenever the saved state changes shape; saves of
  another format are not listed. The end of a campaign writes its log,
  `ArmCom2_Campaign_Log_<time>.txt`, to the data folder (the original
  wrote it beside the game).

`armcom2 --art` browses the 125 pictures, at their own size or fitted to
the screen (`f`). `RUST_LOG=debug armcom2 2>armcom2.log` logs every key,
mode change and a state line each second.

## The design problem: square cells

The original's window is 90x60 cells of 8x8 (or 16x16) pixels: square.
A terminal cell is about twice as tall as wide. So the original's screens
cannot be shown cell for cell: 90x60 square cells are either 180x60
terminal cells (twice the minimum's width, and twice its height) or, in
half blocks, 90x30, where only solid colour survives and text does not.
The screens need laying out again, as I's did. David left the layout to
me (2026-10-19: "since you just finished AC-I I think you have a good idea
of what you are working on"); the decisions and reasons, to be looked at
in play:

- **Pages:** every screen that is not a map (menus, the calendar,
  selection screens, the crewman report, windows) is laid out on a page of
  the minimum screen, 100x30, centred on bigger screens (I's windows did
  the same). Pop-ups (messages, notifications, text input, the game menu,
  the skill menu) are centred on the screen over the darkened page. The
  maps will use the whole terminal, as I's do.
- **Text is one character a column**, so the original's text panels keep
  their widths (25 columns for unit info and crew), and the original's
  rows are packed into 30: blank rows go, frames become the page's own.
- **Art goes through `art.rs`**: portraits (24x8 square cells) are 24x4
  in the terminal, half their own size, which keeps their proportions and
  puts their markings in as characters; flags (29x14) are 29x7.
- **`DisplayMyInfo`** (unit info) is 25x11, 15 with status: the portrait
  four rows instead of eight. **`DisplayCrew`** is four rows a position,
  the original's five less its blank row, with first name and surname on
  one row: six positions in 24 rows.
- **The calendar:** the day outline (24x17) and command menu (24x10) in
  the left column as in the original, the main panel 73x28 beside them;
  the frame is drawn as lines, as `calendar_bkg.xp` is only lines. The
  sunrise gradient is the original's 58 rows scaled to 28.
- **Campaign and tank selection:** the list on the left, the selected one
  in a framed middle column, the options (campaign) or the tank's
  description (tank) in the right column; when replacing a tank, the
  current one at the top right.
- **Crewman report:** commands on the left, the report in the middle
  (skills scroll if there are more than eight), level, skill description
  and injuries on the right.
- **Ammunition load:** the stores drawn 20 shells to a row instead of 9
  (the biggest player gun, 180 rounds and its 15% extra, is 11 rows), the
  ready rack 10 to a row.
- **The battle** (`src/battle_ui.rs`) uses the whole terminal: Armoured
  Commander I's terminal hexes for the map (59x29 at 100x30; 81x43 or
  103x57 when there is room), a 23-column left column and an 18-column
  right column, both a few columns wider on wider screens. The left
  column has the player's tank in two lines (its full `DisplayMyInfo`
  from 45 rows up, and Tab shows it in a window at any size: Tab is not
  in the original), the crew at three rows a position (the original's
  four less its dashed line), and the commands. The right column has the
  date, clock, phase and spotted status, the context panel, the weather in
  three lines, and the unit info (the original's strip under the map):
  the hex under the mouse, else the selected target. Hexes are dotted
  outlines as `scen_hex.xp`, blue for the field of view or a weapon's
  arc; units, their terrain, smoke, guns and cover are drawn at the
  original's offsets from the hex centre (terrain spread twice as wide).
  The attack window, which replaced the original's left column, is a
  54x26 window over the map: attacker, description and target on the
  left, base chance, modifiers, the final chance's bar, the roll and the
  result on the right. The firepower window and the bail-out screen are
  laid out the same way, smaller. Bearings and facings are still worked
  out on the original's square-cell plot (`battle::plot_sq`), so they
  come out as they did.
- **The day map** (`src/daymap.rs`) uses the whole terminal: the command
  menu in a 27-column left column, time, weather, the mission and the
  zone info in a 29-column right column, the map between. Its 41
  pointy-topped zones (the original's are 7x9 square cells, 6 by 5 apart)
  are terminal hexes `c` columns and `r` rows apart, the biggest that fit
  of 8x3 (at 100x30), 10x4, 12x5 and 14x6; each cell belongs to the zone
  its place rounds to (cube rounding, as `GetHexLine`), so they tile
  exactly at any size. At 3 rows a zone they look like offset bricks
  rather than hexes (each still meets its six neighbours); from 4 rows
  their edges zigzag as hexes'. Edges are dotted as the original's; rivers
  colour the edge cells between two zones, bridges cross the middle of
  them, roads run centre to centre, the front line is red squares on the
  edges, terrain is drawn from each zone's seed. The tank's info is above
  the command menu from 41 rows up; at 30 rows it is in the Group tab
  (the crew tab needs the whole column). The zone info follows the mouse
  over the map as in the original; without the mouse it shows the chosen
  direction's zone, else the player's (the original asked for a mouse).
  The travel tab's direction rose is three rows instead of five.

The original's screens, from its consoles:

- **Scenario (the battle):** a 25-column left column (player tank 25x18,
  crew 25x24, command menu 25x12); the hex map 53x43 in the middle, its
  hexes drawn 7x5 cells (`scen_hex.xp` 11x7 with its outline); a right
  column (context 18x12, time 21x6, weather 18x12); a unit info strip
  61x5 under the map; the attack window 27x60.
- **Campaign day:** the day map 35x53 of 41 pointy-topped hexes
  (`CAMPAIGN_DAY_HEXES`, 7x9 cells each), a 25-column left column (player
  unit 25x16, commands 25x41), a 23-column right column (time, weather
  18x12, campaign 23x5, hex info 23x35).
- **Campaign calendar:** day outline 24x36, commands 24x21, main panel
  63x58.
- Windows: the game menu 84x54, the skill menu 43x36, crewman menus at
  full size, pop-ups.

The battle map is the same shape as I's encounter map: the player's hex
and three rings around it, 37 flat-topped hexes (`GetHexDistance <= 3`;
a fourth ring is generated but not shown), plotted `x = hx*7 + 26`,
`y = hy*6 + hx*3 + 21` in square cells. I's terminal hexes (`hex_k`, 59x29
at 100x30, 81x43 or 103x57 on bigger screens) fit it as they are.

## Differences from the original

The rule, as for I (David, 2026-10-18): change what only affects display
or stability; keep what changes the game's odds, and list it here for
David to decide. In commits, the README and the catalog these are
described as what the port does, never as faults in Scott's code (David,
2026-10-19, for I).

Changed (stability, display):

- Canada's Best has a refitting week starting on a combat week's last day
  (1945.03.04), so the date can be in the calendar twice; the calendar
  steps from the first copy to the second, the same date, and never moves
  on. The port steps on from the last copy.
- With Player Commander off, ending a day crashed (`commander_in_a_coma`
  was only set with it on, and read either way).
- The field hospital tab's crewman menu with nobody in hospital crashed.
- Scrolling the journal up from the top crashed (an unset length), and
  scrolling clamped to the entries less 50, so it never scrolled; it
  scrolls within the entries now.
- `DoCrewReplacementCheck`'s second pass, for crewmen displaced by those
  going back to their own positions, crashed (it read a crewman's
  `.crewman`); each goes back to his own position, or an empty one.
- `GenerateMission`'s fallback read `self.current_week` (a campaign day
  has none): the first mission instead. Not reachable with the shipped
  campaigns.
- `GetHexLine` divided by the distance, so a line from a hex to itself
  crashed; it is that hex.
- Birthdays were `choice(monthrange(...))`, the month's first weekday or
  its length, so day 0 to 6 or the last day; any day of the month now.
- Resupply with a dead crewman aboard crashed (it updated battle
  screens a campaign day does not have).
- Off-map road stubs: the original counted only the road links it drew
  from a zone (three of six directions), so it drew stubs off the map edge
  for zones with two roads too, in the previous zone's colour, and could
  crash on the first; here an edge zone with one road gets one (display).
- The commander check after a battle reads the first position's crewman,
  which crashed if the position was empty.
- In battle: the Light Artillery Car's locomotive read a week's
  `enemy_nation` (no such key) and crashed; APCR and APDS at long range
  passed two arguments to `append` and crashed; a crewman's critical
  injury that killed him named an unset `location` and crashed, before a
  battle (`DoInjuryCheck`) and after it (`DoCrewRecoveryCheck`, which
  also called battle code with no battle); the bail-out screen wrapped a
  `False` wound result as text and crashed (it shows OK).
- Crew names went through IBM850, which the font does not follow, so some
  letters came out wrong (`glyph::encode_name`).

Kept as the original (they change the odds or the course of a
campaign), for David:

- A crewman with no injuries is sent to the field hospital one time in
  1001 (`roll <= 0.0` when the roll is 0.0), for 0 days; he returns
  next day (the returning crewman screen), but if he is the player
  commander with Player Commander on, the commander's hospital stay jumps
  the calendar ahead (below).
- `CommanderInAComa` rolls the length of the stay and then releases the
  commander ten days on regardless; it walks the calendar without moving
  the date, so the campaign week is not advanced (it catches up one week
  a day after), and it lands on the combat day before the first one past
  the release date (which can be the same day again).
- A new crewman's stats come from `sample(range(3), 2)` of
  `['Perception', 'Morale', 'Grit', 'Knowledge']`: two of the first three
  start at 3, Knowledge never.
- `TankSelectionMenu` reads each tank's rarity and does nothing with it:
  every tank on the campaign's list is always offered.
- After a refit the calendar moves on twice from the same day, landing on
  the refitting day again; the next Enter moves on from it (the refit day
  is logged with the previous day's records).
- Swapping positions does not change a crewman's CE or BU status until
  something next sets it.
- New crewmen in a squad unit get no national skills (only the player's
  unit does), and a returning crewman keeps his hospital stay noted.
- On the day map: allied and enemy zone captures look for a neighbour in
  five of the six directions (`range(5)`). A new map region keeps the
  player's row with its roads and rivers, now facing other zones, and
  makes the rest without roads or rivers (the original never generates
  them again that day). A zone the enemy takes from under the player is
  captured back after the battle with its VP again. An increase of enemy
  strength in an unknown zone does not reset the random event chance.
- In battle:
  - A grazing hit compares the condition with a list
    (`!= ['Good Order', 'Stunned']`, always true), so it never shakes or
    stuns.
  - `DoCrewRecoveryCheck` sets the campaign day's condition instead of
    the crewman's, so crewmen do not recover after a battle (they do at
    the end of the day).
  - Snipers: the check for the player's position compares a position with
    names, never equal, so fate points never stop a sniper and the
    message always names the position; the heavy rain and blizzard
    modifier compares with a list, never equal.
  - Enemy artillery passes over the player's side (it skips units of side
    0 whoever is firing), so it never harms the player.
  - The AI sets its chosen ammunition on a misspelt attribute, so a gun
    fires the type it was last set to while the AI scored its options;
    and after turning to fire, it fires its last weapon instead of the
    chosen one (a loop reuses the variable's name).
  - A turret's rotation never clears its other weapons' acquired targets
    (`GetStat('Mount')`, a stat no weapon has).
  - Units act in a loop over a list they can leave, by index, so a unit
    that knocks out one listed before it acts again (as Armoured Commander
    I's bug 7, which David kept there).
  - A shot kept by rate of fire is not checked for ammunition again, so
    the stores can go below zero.

Notes from the first look:

- `armcom2.py` by size: `Campaign` (calendar and selection menus) 2,000
  lines, `CampaignDay` (the day map) 3,300, `Personnel`, `Position`,
  `Weapon`, `Unit` and `AI` 4,600, `Scenario` (the battle) 5,000, the
  rest (menus, hex geometry, saves, sound, main menu) 2,500.
- The definitions are JSON (`unit_type_defs.json`, `nation_defs.json`,
  `skill_defs.json`, `crew_command_defs.json`, the campaigns), so there
  is no conversion step as there was for I's Python definitions.
- The original supports keyboard layouts (`KEYBOARDS`: QWERTY, AZERTY,
  QWERTZ, Dvorak, custom; `DeKey`/`EnKey` and `keyboard_mapping.json`),
  remapping its command keys.
- Saves are a folder per campaign (`saved_campaigns/`), several at once,
  with Continue loading the most recent; I had one.
- The original never uses libtcod's colour controls (it prints command
  keys as separate text in `ACTION_KEY_COL`); the console still treats
  codes 1 to 5 and 8 in printed strings as I's colour controls, which is
  harmless as II only places those symbols with `put_char`.

## Tests

`cargo test`: the glyph table and its measured patterns, every art file
loads at its size, the art renderer, the menu fitting 100x30, every
definition loads (and in order), every unit type builds with its crew and
default loads, the hex geometry, every campaign's calendar and days (each
day map can be crossed, roads link both ways), saves loading back the
same, scripted crew screens, the day map's zones tiling at three screen
sizes, 30 days on the map played by an autopilot to their end, twelve
battles played with every screen drawn (to text), saves taken in a
battle loading back the same, and ten whole campaigns from New Campaign to the end of the calendar, headless,
each day played on the map and every battle fought by the autopilot
(`ARMCOM2_CAMPAIGNS=200 cargo test --release campaigns` for more, about a
minute and a half; `ARMCOM2_TRACE=1` prints each campaign's records). Headless runs take
scripted keys (`Game::script`) or make each screen's default choice.

`cargo test --release -- --ignored --test-threads=1` adds whole
campaigns with every screen drawn, battles too, at 100x30, 150x48 and
220x70 (`ARMCOM2_CAMPAIGNS=n` per size; it changes the global screen
size, so it runs alone).

`ARMCOM2_SHOTS=<dir> cargo test shot` writes the calendar, selection,
crew and day map screens as text, to look at their layouts without a terminal.

## Order of work

1. Done: shared layer from I, glyphs, art, main menu.
2. Done: the terminal layout (above).
3. Done: the definitions, the model, New Campaign, the calendar, saves,
   settings, the campaign day map, the battle.
4. Done: sound and the theme tune.
5. Release (2026-10-20): `.github/workflows/release.yml`, on the pattern
   of I's: glibc Linux on Ubuntu 22.04 (x86_64, aarch64), macOS and
   Windows (MSVC), archives `armoured-commander-ii-<target>` with
   `armcom2`, README.md and LICENSE. Tags `v<original version>-av.<n>`;
   the first is to be `v3.0.2-av.1`. Not pushed yet: the token here has
   no `workflow` scope (`gh auth refresh -s workflow`), and Actions are
   blocked by billing (I's notes, since 2026-10-12). It waits on the local
   branch `release-workflow`. Still to do for the catalog: the entry in
   `hub/catalog`, and a preview recording as I's (`asciiverse/`).

## House conventions

- Session logging (`log` + `env_logger`, see `../../CLAUDE.md`).
- Saves go to the platform's data folder, not the install folder.
- Release builds by a workflow on the pattern of I's
  (`../armoured-commander/.github/workflows/release.yml`), glibc instead
  of musl on Linux (see `src/sound.rs` above).
- Every decision here is to be looked at against Armoured Commander I's,
  and the shared layer kept the same where that costs nothing.
