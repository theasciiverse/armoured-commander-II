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
the sweeping light) work. Nothing of the game itself yet.

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
- Sounds (`sounds/*.ogg`, GPL with the rest, and a theme tune) are not
  brought over yet; I's `sound.rs` (rodio) will need its Vorbis feature.
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
  (`AnimateMainMenu`). Continue and Load are greyed (no saves yet); New
  Campaign and Options say they are not ported.

`armcom2 --art` browses the 125 pictures, at their own size or fitted to
the screen (`f`). `RUST_LOG=debug armcom2 2>armcom2.log` logs every key,
mode change and a state line each second.

## The design problem: square cells

The original's window is 90x60 cells of 8x8 (or 16x16) pixels: square.
A terminal cell is about twice as tall as wide. So the original's screens
cannot be shown cell for cell: 90x60 square cells are either 180x60
terminal cells (twice the minimum's width, and twice its height) or, in
half blocks, 90x30, where only solid colour survives and text does not.
The screens need laying out again, as I's did, and the layout is to be
agreed with David before the game's screens are translated.

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
at 100x30, 81x43 or 103x57 on bigger screens) fit it as they are; their
spacing is a little narrower for its height than II's 7 by 6. The squeeze
is the text: the left column stacks tank, crew and commands to 54 rows,
which at 30 rows must be compacted as I's tank and crew panel was, and
59 columns of map plus II's 25 and 18 to 21-column columns is more than
100.

What carries over from I's layout work: the text panels are text, one
character a column, so the 25-column columns fit as they are (I's left
panel is 40); the hex maps get terminal hexes drawn for the cell shape,
growing with the screen (I's `hex_k` scales); art goes through `art.rs`.
A first proposal, for David:

- **Scenario at 100x30:** left column 26 (tank, crew, commands), right
  column 22 (context, time, weather), the hex map between them, ~50x29,
  in terminal hexes like I's; the unit strip as the map's bottom rows.
  Bigger screens grow the map and the text columns' length.
- **Campaign day:** the same two columns either side of the day map, its
  pointy-topped hexes redrawn for the terminal (they are 7x9 square cells,
  about 14x9 terminal cells at their own shape; the 41 hexes make a map
  ~5 hexes wide and 9 tall, which wants scrolling or smaller hexes at 30
  rows).
- **Calendar and windows:** text, laid out in 100x30 and centred on
  bigger screens, with the art (portraits, flags) through `art.rs`.

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
loads at its size, the art renderer's pixels, transparency and fitting,
the menu fitting 100x30, and the tank portraits being there.

## Order of work

1. Done: shared layer from I, glyphs, art, main menu.
2. Agree the terminal layout of the scenario, campaign day and calendar
   screens (see the design problem above).
3. The definitions (JSON) and the rules, in the order the original runs:
   new campaign, calendar, campaign day, scenario.
4. Saves (several, per campaign folder), settings, sound and the theme
   tune, keyboard layouts.

## House conventions

- Session logging (`log` + `env_logger`, see `../../CLAUDE.md`).
- Saves go to the platform's data folder, not the install folder.
- Release builds by the same GitHub workflow as Greed
  (`../greed/.github/workflows/release.yml`).
- Every decision here is to be looked at against Armoured Commander I's,
  and the shared layer kept the same where that costs nothing.
