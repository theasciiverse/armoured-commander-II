//! Glyph codes to Unicode, for II's font.
//!
//! The original draws with its own font, `c64_8x8_ext.png` (or the same at
//! 16x16), 16 glyphs a row and 18 rows: 288 glyphs of square cells. Codes 0
//! to 255 follow CP437 except where the font draws something else, checked
//! glyph by glyph against its bitmaps; 256 to 273 are Polish letters, which
//! `DisplayName` maps crew names onto; 274 to 287 are empty.
//!
//! `glyph_table.rs` has each glyph's 2x2 pixel pattern, measured from the
//! font by `tools/glyphs.py`, for drawing the art (see `art.rs`).

#[path = "glyph_table.rs"]
mod table;
pub use table::QUADS;

/// Glyph codes run to 287.
pub const GLYPHS: usize = 288;

const TABLE: [char; GLYPHS] = [
    // 0x00: CP437's symbols; the font draws 16, 17, 30 and 31 as corner
    // triangles and 28, 29 as corner brackets
    ' ', '☺', '☻', '♥', '♦', '♣', '♠', '●', '◘', '○', '◙', '♂', '♀', '♪', '♫', '☼',
    '◢', '◤', '↕', '‼', '¶', '§', '▬', '↨', '↑', '↓', '→', '←', '└', '┘', '◥', '◣',
    // 0x20: ASCII
    ' ', '!', '"', '#', '$', '%', '&', '\'', '(', ')', '*', '+', ',', '-', '.', '/',
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', ':', ';', '<', '=', '>', '?',
    '@', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O',
    'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', '[', '\\', ']', '^', '_',
    '`', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o',
    'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z', '{', '|', '}', '~', '⌂',
    // 0x80: CP437's accented letters and symbols
    'Ç', 'ü', 'é', 'â', 'ä', 'à', 'å', 'ç', 'ê', 'ë', 'è', 'ï', 'î', 'ì', 'Ä', 'Å',
    'É', 'æ', 'Æ', 'ô', 'ö', 'ò', 'û', 'ù', 'ÿ', 'Ö', 'Ü', '¢', '£', '¥', '₧', 'ƒ',
    'á', 'í', 'ó', 'ú', 'ñ', 'Ñ', 'ª', 'º', '¿', '⌐', '¬', '½', '¼', '¡', '«', '»',
    // 0xB0: shades and box drawing; 178 is a two-quarter checker here
    '░', '▒', '▚', '│', '┤', '╡', '╢', '╖', '╕', '╣', '║', '╗', '╝', '╜', '╛', '┐',
    '└', '┴', '┬', '├', '─', '┼', '╞', '╟', '╚', '╔', '╩', '╦', '╠', '═', '╬', '╧',
    '╨', '╤', '╥', '╙', '╘', '╒', '╓', '╫', '╪', '┘', '┌', '█', '▄', '▌', '▐', '▀',
    // 0xE0: quarter blocks, diagonal arrows, then rounded corners (drawn as
    // the nearest triangles), then CP437 again with 249 a small square and
    // 254 a vertical bar
    '▘', '▝', '▗', '▖', '↗', '↘', '↙', '↖', '∧', '∨', 'Ω', 'δ', '◢', '◣', '◥', '◤',
    '≡', '±', '≥', '≤', '⌠', '⌡', '÷', '≈', '°', '▪', '·', '√', 'ⁿ', '²', '▐', ' ',
    // 0x100: the Polish letters `DisplayName` maps names onto
    'Ą', 'Ć', 'Ę', 'Ł', 'Ń', 'Ó', 'Ś', 'Ź', 'Ż', 'ą', 'ć', 'ę', 'ł', 'ń', 'ó', 'ś',
    'ź', 'ż', ' ', ' ', ' ', ' ', ' ', ' ', ' ', ' ', ' ', ' ', ' ', ' ', ' ', ' ',
];

pub fn to_char(code: u16) -> char {
    TABLE.get(code as usize).copied().unwrap_or('?')
}

/// libtcod's named characters that II uses.
#[allow(dead_code)]
pub mod named {
    pub const BULLET: u16 = 7;
    pub const BLOCK1: u16 = 176;
    pub const BLOCK2: u16 = 177;
    pub const BLOCK3: u16 = 178;
}

/// The game's own text to glyph codes: the original passes strings to
/// libtcod, which takes a character below 256 as its own code (`chr(196)`
/// for a line), so they are written that way here too. Anything above 255
/// is looked up by its letter, `?` if the font has none.
pub fn encode(text: &str) -> Vec<u16> {
    text.chars()
        .map(|c| if (c as u32) < 256 { c as u32 as u16 } else { lookup(c) })
        .collect()
}

/// Text from the data (names of people and places, in Unicode) to glyph
/// codes, each letter looked up in the font. The original did this only
/// for crew names, through the Polish letters and then IBM850, a code page
/// the font does not follow, so some names came out wrong.
pub fn encode_name(text: &str) -> Vec<u16> {
    text.chars().map(lookup).collect()
}

fn lookup(c: char) -> u16 {
    if (c as u32) < 128 {
        return c as u32 as u16;
    }
    TABLE.iter().skip(128).position(|&t| t == c).map_or(b'?' as u16, |p| p as u16 + 128)
}

/// Glyphs that are pixels rather than drawings: blank, blocks, quarter
/// blocks, the checker, corner triangles. Art made of these scales by its
/// 2x2 pattern; other glyphs (letters, symbols) are drawn as themselves
/// where the art is at its own size.
pub fn is_pixels(code: u16) -> bool {
    matches!(code, 0 | 32 | 16 | 17 | 30 | 31 | 178 | 219..=227 | 236..=239 | 255)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_is_itself() {
        for c in 32u16..127 {
            assert_eq!(to_char(c) as u32, c as u32);
        }
    }

    #[test]
    fn polish_letters_have_their_glyphs() {
        assert_eq!(encode_name("Łódź"), vec![259, 162, 100, 272]);
        assert_eq!(encode_name("Väinö"), vec![86, 132, 105, 110, 148]);
        // the game's own text: characters below 256 are their own codes
        assert_eq!(encode("\u{c4}Ż"), vec![196, 264]);
    }

    #[test]
    fn blocks_measure_as_their_shapes() {
        assert_eq!(QUADS[219], 0b1111);
        assert_eq!(QUADS[220], 0b1100);
        assert_eq!(QUADS[221], 0b0101);
        assert_eq!(QUADS[222], 0b1010);
        assert_eq!(QUADS[223], 0b0011);
        assert_eq!(QUADS[224], 0b0001);
        assert_eq!(QUADS[226], 0b1000);
        assert_eq!(QUADS[0], 0);
    }
}
