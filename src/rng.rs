//! A tiny seedable PRNG (SplitMix64). Seedable so tests are deterministic.

#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed ^ 0x9E37_79B9_7F4A_7C15)
    }

    /// Seeds from the system clock.
    pub fn from_time() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x1234_5678);
        Rng::new(nanos)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in [0, 1).
    pub fn f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Uniform in [lo, hi).
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f32()
    }

    /// True with probability `p`.
    pub fn chance(&mut self, p: f32) -> bool {
        self.f32() < p
    }

    /// Uniform in 0..n (n > 0).
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    /// Fisher–Yates shuffle.
    pub fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            v.swap(i, self.below(i + 1));
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn shuffle_is_a_permutation_and_varies() {
        let mut r = Rng::new(5);
        let mut a: Vec<usize> = (0..13).collect();
        r.shuffle(&mut a);
        let mut sorted = a.clone();
        sorted.sort();
        assert_eq!(sorted, (0..13).collect::<Vec<_>>());
        let mut b: Vec<usize> = (0..13).collect();
        r.shuffle(&mut b);
        assert_ne!(a, b);
    }

    use super::*;

    #[test]
    fn same_seed_same_sequence() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn f32_stays_in_unit_interval() {
        let mut r = Rng::new(1);
        let mut sum = 0.0;
        for _ in 0..10_000 {
            let x = r.f32();
            assert!((0.0..1.0).contains(&x));
            sum += x;
        }
        let mean = sum / 10_000.0;
        assert!((mean - 0.5).abs() < 0.02, "mean {mean}");
    }
}
