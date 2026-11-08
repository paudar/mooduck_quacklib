// Adapted from the Apache-2.0 Microduck reference; see NOTICE.
use std::f64::consts::TAU;

pub(crate) struct Rng {
    state: [u64; 4],
    spare: Option<f64>,
}

impl Rng {
    pub fn new(seed: u32) -> Self {
        let mut x = u64::from(seed);
        let state = std::array::from_fn(|_| {
            x = x.wrapping_add(0x9E3779B97F4A7C15);
            let mut z = x;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
            z ^ (z >> 31)
        });
        Self { state, spare: None }
    }

    pub fn next(&mut self) -> u64 {
        let s = &mut self.state;
        let result = s[0].wrapping_add(s[3]).rotate_left(23).wrapping_add(s[0]);
        let t = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(45);
        result
    }

    pub fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1_u64 << 53) as f64
    }

    pub fn range(&mut self, low: f64, high: f64) -> f64 {
        low + (high - low) * self.unit()
    }

    pub fn normal(&mut self) -> f64 {
        if let Some(value) = self.spare.take() {
            return value;
        }
        let radius = (-2.0 * (1.0 - self.unit()).ln()).sqrt();
        let angle = TAU * self.unit();
        self.spare = Some(radius * angle.sin());
        radius * angle.cos()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_reference_stream() {
        let mut rng = Rng::new(crate::MICRODUCK_SEED);
        let expected = [
            11949385093253310428,
            239023398869202814,
            9114320262739296887,
            16718627008839421888,
        ];
        for value in expected {
            assert_eq!(rng.next(), value);
        }
    }
}
