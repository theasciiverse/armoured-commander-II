//! Dice. ArmCom rolls with both libtcod's default generator
//! (`random_get_int(0, lo, hi)`) and Python's `random` module, both seeded
//! from the clock, so no two games are alike and there is nothing to replay
//! against. One small generator (xorshift64*) stands in for both; the
//! port keeps every roll's range and odds, not its sequence.

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.max(1))
    }

    pub fn from_clock() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1);
        Rng::new(nanos ^ 0x9E37_79B9_7F4A_7C15)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// `libtcod.random_get_int(0, lo, hi)` and `random.randint(lo, hi)`:
    /// both ends included.
    pub fn int(&mut self, lo: i32, hi: i32) -> i32 {
        let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
        let span = (hi - lo) as u64 + 1;
        lo + (self.next() % span) as i32
    }

    /// `random.choice`.
    pub fn choice<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.int(0, items.len() as i32 - 1) as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int_covers_both_ends() {
        let mut rng = Rng::new(7);
        let mut seen = [false; 6];
        for _ in 0..1000 {
            let n = rng.int(3, 8);
            assert!((3..=8).contains(&n));
            seen[(n - 3) as usize] = true;
        }
        assert!(seen.iter().all(|&s| s));
    }
}
