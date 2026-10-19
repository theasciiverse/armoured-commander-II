//! libtcod colours: all the named ones, with the values from the libtcodpy
//! II shipped with, and libtcod's colour arithmetic. Taken over from I.
#![allow(dead_code)]

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Color(pub u8, pub u8, pub u8);

// libtcod's named colours, from the libtcodpy_local II shipped
pub const BLACK: Color = Color(0, 0, 0);
pub const DARKEST_GREY: Color = Color(31, 31, 31);
pub const DARKER_GREY: Color = Color(63, 63, 63);
pub const DARK_GREY: Color = Color(95, 95, 95);
pub const GREY: Color = Color(127, 127, 127);
pub const LIGHT_GREY: Color = Color(159, 159, 159);
pub const LIGHTER_GREY: Color = Color(191, 191, 191);
pub const LIGHTEST_GREY: Color = Color(223, 223, 223);
pub const DARKEST_GRAY: Color = Color(31, 31, 31);
pub const DARKER_GRAY: Color = Color(63, 63, 63);
pub const DARK_GRAY: Color = Color(95, 95, 95);
pub const GRAY: Color = Color(127, 127, 127);
pub const LIGHT_GRAY: Color = Color(159, 159, 159);
pub const LIGHTER_GRAY: Color = Color(191, 191, 191);
pub const LIGHTEST_GRAY: Color = Color(223, 223, 223);
pub const WHITE: Color = Color(255, 255, 255);
pub const DARKEST_SEPIA: Color = Color(31, 24, 15);
pub const DARKER_SEPIA: Color = Color(63, 50, 31);
pub const DARK_SEPIA: Color = Color(94, 75, 47);
pub const SEPIA: Color = Color(127, 101, 63);
pub const LIGHT_SEPIA: Color = Color(158, 134, 100);
pub const LIGHTER_SEPIA: Color = Color(191, 171, 143);
pub const LIGHTEST_SEPIA: Color = Color(222, 211, 195);
pub const RED: Color = Color(255, 0, 0);
pub const FLAME: Color = Color(255, 63, 0);
pub const ORANGE: Color = Color(255, 127, 0);
pub const AMBER: Color = Color(255, 191, 0);
pub const YELLOW: Color = Color(255, 255, 0);
pub const LIME: Color = Color(191, 255, 0);
pub const CHARTREUSE: Color = Color(127, 255, 0);
pub const GREEN: Color = Color(0, 255, 0);
pub const SEA: Color = Color(0, 255, 127);
pub const TURQUOISE: Color = Color(0, 255, 191);
pub const CYAN: Color = Color(0, 255, 255);
pub const SKY: Color = Color(0, 191, 255);
pub const AZURE: Color = Color(0, 127, 255);
pub const BLUE: Color = Color(0, 0, 255);
pub const HAN: Color = Color(63, 0, 255);
pub const VIOLET: Color = Color(127, 0, 255);
pub const PURPLE: Color = Color(191, 0, 255);
pub const FUCHSIA: Color = Color(255, 0, 255);
pub const MAGENTA: Color = Color(255, 0, 191);
pub const PINK: Color = Color(255, 0, 127);
pub const CRIMSON: Color = Color(255, 0, 63);
pub const DARK_RED: Color = Color(191, 0, 0);
pub const DARK_FLAME: Color = Color(191, 47, 0);
pub const DARK_ORANGE: Color = Color(191, 95, 0);
pub const DARK_AMBER: Color = Color(191, 143, 0);
pub const DARK_YELLOW: Color = Color(191, 191, 0);
pub const DARK_LIME: Color = Color(143, 191, 0);
pub const DARK_CHARTREUSE: Color = Color(95, 191, 0);
pub const DARK_GREEN: Color = Color(0, 191, 0);
pub const DARK_SEA: Color = Color(0, 191, 95);
pub const DARK_TURQUOISE: Color = Color(0, 191, 143);
pub const DARK_CYAN: Color = Color(0, 191, 191);
pub const DARK_SKY: Color = Color(0, 143, 191);
pub const DARK_AZURE: Color = Color(0, 95, 191);
pub const DARK_BLUE: Color = Color(0, 0, 191);
pub const DARK_HAN: Color = Color(47, 0, 191);
pub const DARK_VIOLET: Color = Color(95, 0, 191);
pub const DARK_PURPLE: Color = Color(143, 0, 191);
pub const DARK_FUCHSIA: Color = Color(191, 0, 191);
pub const DARK_MAGENTA: Color = Color(191, 0, 143);
pub const DARK_PINK: Color = Color(191, 0, 95);
pub const DARK_CRIMSON: Color = Color(191, 0, 47);
pub const DARKER_RED: Color = Color(127, 0, 0);
pub const DARKER_FLAME: Color = Color(127, 31, 0);
pub const DARKER_ORANGE: Color = Color(127, 63, 0);
pub const DARKER_AMBER: Color = Color(127, 95, 0);
pub const DARKER_YELLOW: Color = Color(127, 127, 0);
pub const DARKER_LIME: Color = Color(95, 127, 0);
pub const DARKER_CHARTREUSE: Color = Color(63, 127, 0);
pub const DARKER_GREEN: Color = Color(0, 127, 0);
pub const DARKER_SEA: Color = Color(0, 127, 63);
pub const DARKER_TURQUOISE: Color = Color(0, 127, 95);
pub const DARKER_CYAN: Color = Color(0, 127, 127);
pub const DARKER_SKY: Color = Color(0, 95, 127);
pub const DARKER_AZURE: Color = Color(0, 63, 127);
pub const DARKER_BLUE: Color = Color(0, 0, 127);
pub const DARKER_HAN: Color = Color(31, 0, 127);
pub const DARKER_VIOLET: Color = Color(63, 0, 127);
pub const DARKER_PURPLE: Color = Color(95, 0, 127);
pub const DARKER_FUCHSIA: Color = Color(127, 0, 127);
pub const DARKER_MAGENTA: Color = Color(127, 0, 95);
pub const DARKER_PINK: Color = Color(127, 0, 63);
pub const DARKER_CRIMSON: Color = Color(127, 0, 31);
pub const DARKEST_RED: Color = Color(63, 0, 0);
pub const DARKEST_FLAME: Color = Color(63, 15, 0);
pub const DARKEST_ORANGE: Color = Color(63, 31, 0);
pub const DARKEST_AMBER: Color = Color(63, 47, 0);
pub const DARKEST_YELLOW: Color = Color(63, 63, 0);
pub const DARKEST_LIME: Color = Color(47, 63, 0);
pub const DARKEST_CHARTREUSE: Color = Color(31, 63, 0);
pub const DARKEST_GREEN: Color = Color(0, 63, 0);
pub const DARKEST_SEA: Color = Color(0, 63, 31);
pub const DARKEST_TURQUOISE: Color = Color(0, 63, 47);
pub const DARKEST_CYAN: Color = Color(0, 63, 63);
pub const DARKEST_SKY: Color = Color(0, 47, 63);
pub const DARKEST_AZURE: Color = Color(0, 31, 63);
pub const DARKEST_BLUE: Color = Color(0, 0, 63);
pub const DARKEST_HAN: Color = Color(15, 0, 63);
pub const DARKEST_VIOLET: Color = Color(31, 0, 63);
pub const DARKEST_PURPLE: Color = Color(47, 0, 63);
pub const DARKEST_FUCHSIA: Color = Color(63, 0, 63);
pub const DARKEST_MAGENTA: Color = Color(63, 0, 47);
pub const DARKEST_PINK: Color = Color(63, 0, 31);
pub const DARKEST_CRIMSON: Color = Color(63, 0, 15);
pub const LIGHT_RED: Color = Color(255, 114, 114);
pub const LIGHT_FLAME: Color = Color(255, 149, 114);
pub const LIGHT_ORANGE: Color = Color(255, 184, 114);
pub const LIGHT_AMBER: Color = Color(255, 219, 114);
pub const LIGHT_YELLOW: Color = Color(255, 255, 114);
pub const LIGHT_LIME: Color = Color(219, 255, 114);
pub const LIGHT_CHARTREUSE: Color = Color(184, 255, 114);
pub const LIGHT_GREEN: Color = Color(114, 255, 114);
pub const LIGHT_SEA: Color = Color(114, 255, 184);
pub const LIGHT_TURQUOISE: Color = Color(114, 255, 219);
pub const LIGHT_CYAN: Color = Color(114, 255, 255);
pub const LIGHT_SKY: Color = Color(114, 219, 255);
pub const LIGHT_AZURE: Color = Color(114, 184, 255);
pub const LIGHT_BLUE: Color = Color(114, 114, 255);
pub const LIGHT_HAN: Color = Color(149, 114, 255);
pub const LIGHT_VIOLET: Color = Color(184, 114, 255);
pub const LIGHT_PURPLE: Color = Color(219, 114, 255);
pub const LIGHT_FUCHSIA: Color = Color(255, 114, 255);
pub const LIGHT_MAGENTA: Color = Color(255, 114, 219);
pub const LIGHT_PINK: Color = Color(255, 114, 184);
pub const LIGHT_CRIMSON: Color = Color(255, 114, 149);
pub const LIGHTER_RED: Color = Color(255, 165, 165);
pub const LIGHTER_FLAME: Color = Color(255, 188, 165);
pub const LIGHTER_ORANGE: Color = Color(255, 210, 165);
pub const LIGHTER_AMBER: Color = Color(255, 232, 165);
pub const LIGHTER_YELLOW: Color = Color(255, 255, 165);
pub const LIGHTER_LIME: Color = Color(232, 255, 165);
pub const LIGHTER_CHARTREUSE: Color = Color(210, 255, 165);
pub const LIGHTER_GREEN: Color = Color(165, 255, 165);
pub const LIGHTER_SEA: Color = Color(165, 255, 210);
pub const LIGHTER_TURQUOISE: Color = Color(165, 255, 232);
pub const LIGHTER_CYAN: Color = Color(165, 255, 255);
pub const LIGHTER_SKY: Color = Color(165, 232, 255);
pub const LIGHTER_AZURE: Color = Color(165, 210, 255);
pub const LIGHTER_BLUE: Color = Color(165, 165, 255);
pub const LIGHTER_HAN: Color = Color(188, 165, 255);
pub const LIGHTER_VIOLET: Color = Color(210, 165, 255);
pub const LIGHTER_PURPLE: Color = Color(232, 165, 255);
pub const LIGHTER_FUCHSIA: Color = Color(255, 165, 255);
pub const LIGHTER_MAGENTA: Color = Color(255, 165, 232);
pub const LIGHTER_PINK: Color = Color(255, 165, 210);
pub const LIGHTER_CRIMSON: Color = Color(255, 165, 188);
pub const LIGHTEST_RED: Color = Color(255, 191, 191);
pub const LIGHTEST_FLAME: Color = Color(255, 207, 191);
pub const LIGHTEST_ORANGE: Color = Color(255, 223, 191);
pub const LIGHTEST_AMBER: Color = Color(255, 239, 191);
pub const LIGHTEST_YELLOW: Color = Color(255, 255, 191);
pub const LIGHTEST_LIME: Color = Color(239, 255, 191);
pub const LIGHTEST_CHARTREUSE: Color = Color(223, 255, 191);
pub const LIGHTEST_GREEN: Color = Color(191, 255, 191);
pub const LIGHTEST_SEA: Color = Color(191, 255, 223);
pub const LIGHTEST_TURQUOISE: Color = Color(191, 255, 239);
pub const LIGHTEST_CYAN: Color = Color(191, 255, 255);
pub const LIGHTEST_SKY: Color = Color(191, 239, 255);
pub const LIGHTEST_AZURE: Color = Color(191, 223, 255);
pub const LIGHTEST_BLUE: Color = Color(191, 191, 255);
pub const LIGHTEST_HAN: Color = Color(207, 191, 255);
pub const LIGHTEST_VIOLET: Color = Color(223, 191, 255);
pub const LIGHTEST_PURPLE: Color = Color(239, 191, 255);
pub const LIGHTEST_FUCHSIA: Color = Color(255, 191, 255);
pub const LIGHTEST_MAGENTA: Color = Color(255, 191, 239);
pub const LIGHTEST_PINK: Color = Color(255, 191, 223);
pub const LIGHTEST_CRIMSON: Color = Color(255, 191, 207);
pub const DESATURATED_RED: Color = Color(127, 63, 63);
pub const DESATURATED_FLAME: Color = Color(127, 79, 63);
pub const DESATURATED_ORANGE: Color = Color(127, 95, 63);
pub const DESATURATED_AMBER: Color = Color(127, 111, 63);
pub const DESATURATED_YELLOW: Color = Color(127, 127, 63);
pub const DESATURATED_LIME: Color = Color(111, 127, 63);
pub const DESATURATED_CHARTREUSE: Color = Color(95, 127, 63);
pub const DESATURATED_GREEN: Color = Color(63, 127, 63);
pub const DESATURATED_SEA: Color = Color(63, 127, 95);
pub const DESATURATED_TURQUOISE: Color = Color(63, 127, 111);
pub const DESATURATED_CYAN: Color = Color(63, 127, 127);
pub const DESATURATED_SKY: Color = Color(63, 111, 127);
pub const DESATURATED_AZURE: Color = Color(63, 95, 127);
pub const DESATURATED_BLUE: Color = Color(63, 63, 127);
pub const DESATURATED_HAN: Color = Color(79, 63, 127);
pub const DESATURATED_VIOLET: Color = Color(95, 63, 127);
pub const DESATURATED_PURPLE: Color = Color(111, 63, 127);
pub const DESATURATED_FUCHSIA: Color = Color(127, 63, 127);
pub const DESATURATED_MAGENTA: Color = Color(127, 63, 111);
pub const DESATURATED_PINK: Color = Color(127, 63, 95);
pub const DESATURATED_CRIMSON: Color = Color(127, 63, 79);
pub const BRASS: Color = Color(191, 151, 96);
pub const COPPER: Color = Color(197, 136, 124);
pub const GOLD: Color = Color(229, 191, 0);
pub const SILVER: Color = Color(203, 203, 203);
pub const CELADON: Color = Color(172, 255, 175);
pub const PEACH: Color = Color(255, 159, 127);

/// `TCOD_color_lerp`: each channel `c1 + (c2 - c1) * coef`, truncated.
pub fn lerp(c1: Color, c2: Color, coef: f32) -> Color {
    let ch = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * coef) as u8;
    Color(ch(c1.0, c2.0), ch(c1.1, c2.1), ch(c1.2, c2.2))
}

/// `color1 * color2`: each channel multiplied, over 255.
pub fn mul(c1: Color, c2: Color) -> Color {
    let ch = |a: u8, b: u8| (a as u32 * b as u32 / 255) as u8;
    Color(ch(c1.0, c2.0), ch(c1.1, c2.1), ch(c1.2, c2.2))
}

/// `libtcod.Color(r + n, g + n, b + n)`, kept within 0 to 255.
pub fn shift(c: (i32, i32, i32), n: i32) -> Color {
    let ch = |v: i32| (v + n).clamp(0, 255) as u8;
    Color(ch(c.0), ch(c.1), ch(c.2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lerp_matches_libtcod() {
        // The title sky: row 15 of 30 between (255,255,114) and (193,8,3).
        let c = lerp(Color(255, 255, 114), Color(193, 8, 3), 0.5);
        assert_eq!(c, Color(224, 131, 58));
        assert_eq!(lerp(BLACK, WHITE, 0.0), BLACK);
        assert_eq!(lerp(BLACK, WHITE, 1.0), WHITE);
    }
}
