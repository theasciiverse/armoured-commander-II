# Armoured Commander II

Gregory Adam Scott's World War II tank commander roguelike (2016 to 2020),
ported to the terminal in Rust and Ratatui for the Asciiverse catalog. You
command a tank and its crew through one of ten campaigns, from Poland in
1939 to Germany in 1945: day by day across a map of zones you take and
hold, and battle by battle on a hex map round your tank, giving each
crewman his orders while the dice decide what every shot does.

The original drew its square character cells into its own window through
libtcod. This port keeps the game, its art, its sounds and its theme tune,
and draws in whatever terminal you play in.

## Playing

    armcom2

New Campaign chooses a campaign and a tank; the campaign is saved as it
goes, one save for each campaign. Continue picks up the most recent, Load
any of them. Escape is the game menu (options, Save and Quit); each screen
shows its keys.

    armcom2 --battle [campaign]

goes straight into a battle with a fresh tank and crew, for trying the
battle rules (nothing is saved), and

    armcom2 --art

browses the game's pictures.

- The terminal must be at least 100x30. Bigger terminals get bigger maps.
- The options turn sound on and off and set its volume, the length of the
  pauses for messages, and the keyboard layout (QWERTY, AZERTY, QWERTZ,
  Dvorak). On Linux the game uses ALSA (and through it PipeWire or
  PulseAudio) if it is installed, and plays silently if not.
- Saved campaigns, the settings and the campaign logs are kept in
  `$XDG_DATA_HOME/armoured-commander-ii`
  (`~/.local/share/armoured-commander-ii`), or
  `%APPDATA%\armoured-commander-ii` on Windows.
- `RUST_LOG=debug armcom2 2>armcom2.log` writes a session log.

## Changes from the original

The rules, tables, campaigns, art and sounds are the original's. What is
different:

- Laid out for a terminal, whose cells are twice as tall as wide where the
  original's were square: menus and windows on a 100x30 page, the maps
  using the whole terminal. The day map's zones and the battle's hexes
  are drawn for the terminal and grow with it; portraits are half their
  size; the attack window opens over the map. Tab shows your tank in full
  in a battle. Without a mouse, the zone and unit panels show your chosen
  zone or target.
- Small differences in play and display: a calendar that has a date
  twice moves on from it; a birthday falls on a day of its month; crew
  names keep their letters; the journal scrolls; the field hospital and
  crew replacement work in every case; off-map roads are drawn where a
  zone has one road; in battle, the Light Artillery Car's locomotive
  belongs to the battle's nation, APCR at long range takes its penalty,
  and the bail-out shows every crewman's result; the font size option is
  gone.
- Steam is gone.

## Credits and licence

- Armoured Commander II: Gregory Adam Scott, 2016 to 2020,
  [github.com/sudasana/armcom2](https://github.com/sudasana/armcom2)
- Terminal port: Asciiverse
- GPL-3.0-or-later, as the original (see `LICENSE`). The art is the
  original's REXPaint files, the definitions and campaigns its JSON files,
  and the sounds and theme its Ogg files, unchanged.
