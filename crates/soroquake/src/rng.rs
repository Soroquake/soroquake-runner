//! Deterministic randomness. Every campaign is driven by one seed so any
//! failure can be replayed bit-for-bit.

/// SplitMix64: small, fast and deterministic across platforms.
#[derive(Debug, Clone)]
pub struct SeededRng {
    state: u64,
}

impl SeededRng {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform value in `0..n` (`n` must be non-zero).
    pub fn below(&mut self, n: u64) -> u64 {
        assert!(n > 0, "below(0)");
        self.next_u64() % n
    }

    /// Uniform value in `lo..=hi`.
    pub fn range(&mut self, lo: i128, hi: i128) -> i128 {
        assert!(hi >= lo, "empty range");
        lo + (self.next_u64() as u128 % (hi - lo + 1) as u128) as i128
    }

    /// True with probability `percent`/100.
    pub fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    /// Pick one element of a non-empty slice.
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }

    /// Derive an independent child stream, e.g. one per worker.
    pub fn fork(&mut self) -> Self {
        Self::new(self.next_u64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_sequence() {
        let mut a = SeededRng::new(42);
        let mut b = SeededRng::new(42);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn below_stays_in_range() {
        let mut r = SeededRng::new(7);
        assert!((0..10_000).all(|_| r.below(5) < 5));
    }
}
