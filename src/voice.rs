// Voice personality and DSP adapted from Microduck's Apache-2.0 tools/quack.py.
// See NOTICE for attribution and the differences from the reference.
use crate::{recipes::Call, rng::Rng};
use std::f64::consts::TAU;

pub(crate) struct Voice {
    pub pitch: f64,
    harmonics: [f64; 7],
    vibrato_rate: f64,
    vibrato_depth: f64,
    jitter: f64,
    breath: f64,
    quackiness: f64,
    am_rate: f64,
    am_depth: f64,
    attack_sharpness: f64,
    pub speed: f64,
}

impl Voice {
    pub fn new(seed: u32) -> Self {
        let mut rng = Rng::new(seed);
        let register = [-1.0, 0.0, 0.0, 1.0][(rng.unit() * 4.0) as usize] + rng.range(-0.4, 0.4);
        let pitch = (rng.range(160.0, 380.0) * 2.0_f64.powf(register * 0.45)).clamp(110.0, 620.0);
        // Consume unused personality fields in reference order to preserve identity.
        let _spread = rng.range(0.4, 1.2);
        let _glide = rng.range(-1.0, 1.0);
        let brightness = rng.range(0.05, 0.55);
        let tilt = rng.range(1.4, 2.8);
        let nasal = rng.range(0.1, 1.0);
        let skew = rng.range(-1.0, 1.0);
        let formant = 1 + (rng.unit() * 5.0) as usize;
        let formant_gain = rng.range(0.0, 1.4);
        let harmonics = std::array::from_fn(|i| {
            let n = i + 1;
            let base = 1.0 / (n as f64).powf(tilt);
            let high = brightness * (n as f64 / 7.0).powf(1.5);
            let nasal = if n == 2 || n == 3 { nasal * 0.6 } else { 0.0 };
            let skew = if skew >= 0.0 {
                skew * if n % 2 == 0 { 0.4 } else { -0.2 }
            } else {
                -skew * if n % 2 == 0 { -0.3 } else { 0.4 }
            };
            let formant = if n == formant {
                formant_gain * base * 1.5
            } else {
                0.0
            };
            (base + high + nasal + skew + formant).max(if n == 1 { 0.7 } else { 0.0 })
        });
        let vibrato_rate = rng.range(3.5, 9.5);
        let vibrato_depth = rng.range(0.0, 0.7);
        let jitter = rng.range(0.03, 0.35);
        let breath = rng.range(0.0, 0.30);
        let quackiness = rng.range(0.2, 1.0);
        let am_rate = rng.range(18.0, 55.0);
        let am_depth = rng.range(0.15, 0.70);
        let _warble_rate = rng.range(7.0, 18.0);
        let _warble_depth = rng.range(0.0, 1.4);
        let attack_sharpness = rng.range(0.0, 1.0);
        let speed = rng.range(0.82, 1.22);
        Self {
            pitch,
            harmonics,
            vibrato_rate,
            vibrato_depth,
            jitter,
            breath,
            quackiness,
            am_rate,
            am_depth,
            attack_sharpness,
            speed,
        }
    }

    pub fn render(&self, call: &Call, sample_rate: u32, intensity: f64, rng: &mut Rng) -> Vec<f64> {
        let rate = f64::from(sample_rate);
        let count = (call.duration * rate).round() as usize;
        let vib_phase = rng.range(0.0, TAU);
        // Same smoothing time and leaky-integrator pole as the 48 kHz reference.
        let window = (64.0 * rate / 22_050.0).round().max(1.0) as usize;
        let mut history = vec![0.0; window];
        let mut jitter_sum = 0.0;
        let mut pink = vec![0.0; count];
        let pole = 0.985_f64.powf(22_050.0 / rate);
        let mut state = 0.0;
        let mut noise_peak: f64 = 1e-9;
        for value in &mut pink {
            state = pole * state + rng.normal();
            *value = state;
            noise_peak = noise_peak.max(state.abs());
        }
        let mut phase = 0.0;
        let mut output = Vec::with_capacity(count);
        for (i, noise) in pink.into_iter().enumerate() {
            let t = i as f64 / rate;
            let position = t / call.duration;
            let semitones = call.pitch_at(position) * intensity;
            let base = self.pitch * call.register * 2.0_f64.powf(semitones / 12.0);
            let vibrato =
                self.vibrato_depth * call.wobble * (TAU * self.vibrato_rate * t + vib_phase).sin();
            let j = rng.normal();
            jitter_sum += j - history[i % window];
            history[i % window] = j;
            let jitter = self.jitter * jitter_sum / window as f64;
            let freq = (base * 2.0_f64.powf((vibrato + jitter) / 12.0)).min(rate * 0.4);
            phase = (phase + TAU * freq / rate) % TAU;
            let mut body = 0.0;
            for (h, weight) in self.harmonics.iter().enumerate() {
                let harmonic = (h + 1) as f64;
                // Fade harmonics out before Nyquist, including at low robot sample rates.
                let attenuation = ((rate * 0.49 - freq * harmonic) / (rate * 0.04)).clamp(0.0, 1.0);
                body += weight * attenuation * (phase * harmonic).sin();
            }
            let roughness = (self.am_depth * self.quackiness * call.roughness).min(0.95);
            body *= 1.0 - roughness * (0.5 + 0.5 * (TAU * self.am_rate * t).sin());
            body += self.breath * call.breath * noise / noise_peak;
            let attack = ((0.04 - 0.037 * self.attack_sharpness) * call.duration).max(0.003);
            let attack = (t / attack).min(1.0);
            // Explicit end fade prevents a discontinuity when a call ends mid-cycle.
            let release = ((count - 1 - i) as f64 / (rate * 0.018)).min(1.0);
            let envelope = if call.sustained {
                attack * ((call.duration - t) / (call.duration * 0.35)).min(1.0)
            } else {
                attack * (-t / (call.duration * 0.48)).exp()
            };
            output.push(body * envelope * release * call.gain);
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_personality_matches() {
        // Golden values from the published Python reference, not the Rust implementation.
        let voice = Voice::new(crate::MICRODUCK_SEED);
        assert!((voice.pitch - 237.94991246492208).abs() < 1e-9);
        assert!((voice.vibrato_rate - 6.093622319327004).abs() < 1e-12);
        assert!((voice.quackiness - 0.6254198428261108).abs() < 1e-12);
        assert!((voice.speed - 0.8957994314663678).abs() < 1e-12);
    }
}
