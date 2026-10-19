//! Hex geometry, from the end of armcom2.py: distances, lines, rings,
//! neighbours.
//!
//! `GetCubeCoords` is written for offset coordinates, `hx - (hy - hy&1)/2`,
//! but `&` binds looser than `-` in Python, so that is `(hy - hy) & 1`,
//! nothing: the game's hex coordinates are axial throughout (its
//! neighbour tables, `DESTHEX` and `CD_DESTHEX`, are axial ones), and it
//! all works. The port uses axial coordinates as the game in effect does.

/// `DESTHEX`: the scenario map's neighbours, flat-topped hexes,
/// direction 0 up and on clockwise.
pub const DESTHEX: [(i32, i32); 6] = [(0, -1), (1, -1), (1, 0), (0, 1), (-1, 1), (-1, 0)];

/// `CD_DESTHEX`: the campaign day map's, pointy-topped, direction 0 up and
/// right and on clockwise.
pub const CD_DESTHEX: [(i32, i32); 6] = [(1, -1), (1, 0), (0, 1), (-1, 1), (-1, 0), (0, -1)];

/// `ConstrainDir`
pub fn constrain_dir(d: i32) -> i32 {
    d.rem_euclid(6)
}

/// `GetCubeCoords` as it works out.
pub fn cube(hx: i32, hy: i32) -> (i32, i32, i32) {
    (hx, -hx - hy, hy)
}

/// `GetHexDistance`
pub fn distance(hx1: i32, hy1: i32, hx2: i32, hy2: i32) -> i32 {
    let (x1, y1, z1) = cube(hx1, hy1);
    let (x2, y2, z2) = cube(hx2, hy2);
    ((x1 - x2).abs() + (y1 - y2).abs() + (z1 - z2).abs()) / 2
}

/// `RotateHex`: around 0,0 clockwise `r` times.
pub fn rotate(hx: i32, hy: i32, r: i32) -> (i32, i32) {
    let (mut x, mut y, mut z) = cube(hx, hy);
    for _ in 0..r {
        (x, y, z) = (-z, -x, -y);
    }
    let _ = y;
    (x, z)
}

/// `GetAdjacentHex`
pub fn adjacent(hx: i32, hy: i32, d: i32) -> (i32, i32) {
    let (mx, my) = DESTHEX[constrain_dir(d) as usize];
    (hx + mx, hy + my)
}

/// `GetDirectionToAdjacent`: -1 if not adjacent.
pub fn direction_to_adjacent(hx1: i32, hy1: i32, hx2: i32, hy2: i32) -> i32 {
    DESTHEX.iter().position(|&d| d == (hx2 - hx1, hy2 - hy1)).map_or(-1, |i| i as i32)
}

/// `GetAdjacentCDHex`
pub fn adjacent_cd(hx: i32, hy: i32, d: i32) -> (i32, i32) {
    let (mx, my) = CD_DESTHEX[constrain_dir(d) as usize];
    (hx + mx, hy + my)
}

/// `GetDirectionToAdjacentCD`: `None` if not adjacent.
pub fn direction_to_adjacent_cd(hx1: i32, hy1: i32, hx2: i32, hy2: i32) -> Option<i32> {
    CD_DESTHEX.iter().position(|&d| d == (hx2 - hx1, hy2 - hy1)).map(|i| i as i32)
}

/// `GetHexLine`: the hexes from one to the other, both included. Python
/// rounds halves to even, and so does this. The original divides by the
/// distance, so a line from a hex to itself crashed it; here it is that
/// hex.
pub fn line(hx1: i32, hy1: i32, hx2: i32, hy2: i32) -> Vec<(i32, i32)> {
    let (x1, y1, z1) = cube(hx1, hy1);
    let (x2, y2, z2) = cube(hx2, hy2);
    let dist = distance(hx1, hy1, hx2, hy2);
    if dist == 0 {
        return vec![(hx1, hy1)];
    }
    let lerp = |a: i32, b: i32, t: f64| a as f64 + (b - a) as f64 * t;
    (0..=dist)
        .map(|i| {
            let t = 1.0 / dist as f64 * i as f64;
            let (x, y, z) = (lerp(x1, x2, t), lerp(y1, y2, t), lerp(z1, z2, t));
            let (mut rx, mut ry, mut rz) = (x.round_ties_even(), y.round_ties_even(), z.round_ties_even());
            let (dx, dy, dz) = ((rx - x).abs(), (ry - y).abs(), (rz - z).abs());
            if dx > dy && dx > dz {
                rx = -ry - rz;
            } else if dy > dz {
                ry = -rx - rz;
            } else {
                rz = -rx - ry;
            }
            let _ = ry;
            (rx as i32, rz as i32)
        })
        .collect()
}

/// `GetHexRing`
pub fn ring(hx: i32, hy: i32, radius: i32) -> Vec<(i32, i32)> {
    if radius == 0 {
        return vec![(hx, hy)];
    }
    let (mut hx, mut hy) = (hx - radius, hy + radius);
    let mut list = Vec::new();
    for d in 0..6 {
        for _ in 0..radius {
            list.push((hx, hy));
            (hx, hy) = adjacent(hx, hy, d);
        }
    }
    list
}

/// `GetCoveredHexes`: the hextant in direction `d`, to range 3.
pub fn covered(hx: i32, hy: i32, d: i32) -> Vec<(i32, i32)> {
    let mut list = vec![(hx, hy)];
    let (mut x, mut y) = (hx, hy);
    for _ in 0..2 {
        (x, y) = adjacent(x, y, d);
    }
    list.push((x, y));
    list.extend(ring(x, y, 1));
    list
}

/// `GetDirectionalArrow`
pub fn arrow(d: i32) -> u16 {
    match d {
        0 => 24,
        1 => 228,
        2 => 229,
        3 => 25,
        4 => 230,
        5 => 231,
        _ => b'*' as u16,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distances() {
        assert_eq!(distance(0, 0, 0, 3), 3);
        assert_eq!(distance(0, 0, 3, -3), 3);
        assert_eq!(distance(-4, 8, 4, 0), 8);
        for d in 0..6 {
            let (x, y) = adjacent(0, 0, d);
            assert_eq!(distance(0, 0, x, y), 1);
            let (x, y) = adjacent_cd(0, 0, d);
            assert_eq!(distance(0, 0, x, y), 1);
        }
    }

    #[test]
    fn lines_are_unbroken() {
        for (a, b) in [((-4, 8), (4, 0)), ((0, 0), (2, 5)), ((3, 1), (-2, 7)), ((1, 1), (1, 1))] {
            let l = line(a.0, a.1, b.0, b.1);
            assert_eq!(l[0], a);
            assert_eq!(*l.last().unwrap(), b);
            for w in l.windows(2) {
                assert_eq!(distance(w[0].0, w[0].1, w[1].0, w[1].1), 1);
            }
        }
    }

    #[test]
    fn rings_are_round() {
        for r in 1..4 {
            let ring = ring(0, 0, r);
            assert_eq!(ring.len() as i32, 6 * r);
            assert!(ring.iter().all(|&(x, y)| distance(0, 0, x, y) == r));
        }
    }

    #[test]
    fn rotating_six_times_comes_back() {
        assert_eq!(rotate(2, -1, 6), (2, -1));
        assert_eq!(distance(0, 0, rotate(2, -1, 1).0, rotate(2, -1, 1).1), 2);
    }
}
