//! libtcod colours: the named ones ArmCom uses, with the values from the
//! libtcodpy it shipped with, and libtcod's colour arithmetic. Most are for
//! screens not ported yet.
#![allow(dead_code)]

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Color(pub u8, pub u8, pub u8);

pub const BLACK: Color = Color(0, 0, 0);
pub const BLUE: Color = Color(0, 0, 255);
pub const DARKER_GREY: Color = Color(63, 63, 63);
pub const DARKEST_GREY: Color = Color(31, 31, 31);
pub const DARK_GREY: Color = Color(95, 95, 95);
pub const DARK_RED: Color = Color(191, 0, 0);
pub const DARK_YELLOW: Color = Color(191, 191, 0);
pub const GREEN: Color = Color(0, 255, 0);
pub const GREY: Color = Color(127, 127, 127);
pub const LIGHT_BLUE: Color = Color(114, 114, 255);
pub const LIGHTER_BLUE: Color = Color(165, 165, 255);
pub const LIGHTER_GREY: Color = Color(191, 191, 191);
pub const LIGHT_GREEN: Color = Color(114, 255, 114);
pub const LIGHT_GREY: Color = Color(159, 159, 159);
pub const LIGHT_RED: Color = Color(255, 114, 114);
pub const LIGHT_YELLOW: Color = Color(255, 255, 114);
pub const PURPLE: Color = Color(191, 0, 255);
pub const RED: Color = Color(255, 0, 0);
pub const WHITE: Color = Color(255, 255, 255);
pub const YELLOW: Color = Color(255, 255, 0);

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
