//! A seeded generator, so a run's order and values are reproducible from its seed.

/// SplitMix64.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = (self.next_u64() % (i as u64 + 1)) as usize;
            v.swap(i, j);
        }
    }

    pub fn hex(&mut self, bytes: usize) -> String {
        let mut s = String::with_capacity(bytes * 2);
        while s.len() < bytes * 2 {
            s.push_str(&format!("{:016x}", self.next_u64()));
        }
        s.truncate(bytes * 2);
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_sequence() {
        let (mut a, mut b) = (Rng::new(7), Rng::new(7));
        assert_eq!(a.next_u64(), b.next_u64());
        let mut v1: Vec<u32> = (0..10).collect();
        let mut v2 = v1.clone();
        a.shuffle(&mut v1);
        b.shuffle(&mut v2);
        assert_eq!(v1, v2);
        assert_eq!(Rng::new(1).hex(32).len(), 64);
    }
}
