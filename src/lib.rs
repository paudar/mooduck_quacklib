//! Small, offline, Microduck-inspired expressive voice synthesis.
//!
//! ```
//! use mooduck_quacklib::{Emotion, Options, Synthesizer};
//! let voice = Synthesizer::default();
//! let audio = voice.render(Emotion::Curious, Options {
//!     intensity: 0.8, volume: 0.5, ..Options::default()
//! })?;
//! // Feed audio.samples() (mono f32 PCM) to your robot's audio system.
//! # Ok::<(), mooduck_quacklib::Error>(())
//! ```

mod recipes;
mod rng;
mod voice;
mod wav;

use std::{fmt, str::FromStr};

/// Voice identity used in the published Microduck emotions reference.
pub const MICRODUCK_SEED: u32 = 4_145_077_059;

/// Named expression. Intensity controls its strength; volume controls output gain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Emotion {
    Happy,
    Curious,
    Sad,
    Angry,
    Surprised,
    Yes,
    No,
    Excited,
    Laugh,
    Confused,
}

impl Emotion {
    pub const ALL: [Self; 10] = [
        Self::Happy,
        Self::Curious,
        Self::Sad,
        Self::Angry,
        Self::Surprised,
        Self::Yes,
        Self::No,
        Self::Excited,
        Self::Laugh,
        Self::Confused,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Happy => "happy",
            Self::Curious => "curious",
            Self::Sad => "sad",
            Self::Angry => "angry",
            Self::Surprised => "surprised",
            Self::Yes => "yes",
            Self::No => "no",
            Self::Excited => "excited",
            Self::Laugh => "laugh",
            Self::Confused => "confused",
        }
    }
}

impl fmt::Display for Emotion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Emotion {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_lowercase().as_str() {
            "happy" | "joy" | "freude" | "froh" => Ok(Self::Happy),
            "curious" | "neugier" | "neugierig" => Ok(Self::Curious),
            "sad" | "trauer" | "traurig" => Ok(Self::Sad),
            "angry" | "wut" | "wütend" | "wuetend" => Ok(Self::Angry),
            "surprised" | "überrascht" | "ueberrascht" => Ok(Self::Surprised),
            "yes" | "ja" => Ok(Self::Yes),
            "no" | "nein" => Ok(Self::No),
            "excited" | "aufgeregt" => Ok(Self::Excited),
            "laugh" | "lachen" => Ok(Self::Laugh),
            "confused" | "verwirrt" | "mmh" => Ok(Self::Confused),
            _ => Err(Error(format!(
                "unknown emotion {value:?}; use one of: {}",
                Self::ALL.map(Self::name).join(", ")
            ))),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// Expression strength, 0..=1. Affects pitch movement, duration, call count,
    /// roughness and gain. Low strength produces shorter, simpler expressions;
    /// curiosity gains a second call above 0.5. Zero is subdued, not silence.
    pub intensity: f32,
    /// Linear output gain, 0..=1. Zero is exact silence.
    pub volume: f32,
    /// Mono PCM sample rate, 8,000..=192,000 Hz. Default: 48,000 Hz.
    pub sample_rate: u32,
    /// Repeatable small pitch/timing/noise variation within the same voice.
    pub variant: u32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            intensity: 0.7,
            volume: 0.6,
            sample_rate: 48_000,
            variant: 0,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}

/// An owned, finite, peak-bounded mono clip; rendering is intended off the audio thread.
#[derive(Clone, Debug)]
pub struct Audio {
    samples: Vec<f32>,
    sample_rate: u32,
}

impl Audio {
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }
    pub fn into_samples(self) -> Vec<f32> {
        self.samples
    }
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
    pub fn duration_seconds(&self) -> f64 {
        self.samples.len() as f64 / f64::from(self.sample_rate)
    }

    /// Write mono little-endian 16-bit PCM WAV. Does not normalize or alter volume.
    pub fn write_wav(&self, writer: impl std::io::Write) -> std::io::Result<()> {
        wav::write(self, writer)
    }

    pub fn save_wav(&self, path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
        let mut writer = std::io::BufWriter::new(std::fs::File::create(path)?);
        self.write_wav(&mut writer)?;
        std::io::Write::flush(&mut writer)
    }
}

/// Reusable seeded voice, independent of audio devices and platform libraries.
pub struct Synthesizer {
    seed: u32,
    voice: voice::Voice,
}

impl Default for Synthesizer {
    fn default() -> Self {
        Self::new(MICRODUCK_SEED)
    }
}

impl Synthesizer {
    pub fn new(seed: u32) -> Self {
        Self {
            seed,
            voice: voice::Voice::new(seed),
        }
    }
    pub fn seed(&self) -> u32 {
        self.seed
    }

    pub fn render(&self, emotion: Emotion, options: Options) -> Result<Audio, Error> {
        for (name, value) in [("intensity", options.intensity), ("volume", options.volume)] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(Error(format!("{name} must be finite and between 0 and 1")));
            }
        }
        if !(8_000..=192_000).contains(&options.sample_rate) {
            return Err(Error(
                "sample_rate must be between 8000 and 192000 Hz".into(),
            ));
        }
        let strength = f64::from(options.intensity);
        let mut calls = recipes::recipe(emotion, strength, self.voice.speed, self.voice.pitch);
        // Stable explicit IDs, never Rust's unspecified hash implementation.
        let tag = Emotion::ALL.iter().position(|e| *e == emotion).unwrap() as u32;
        let mut rng = rng::Rng::new(
            self.seed ^ tag.wrapping_mul(0x9E3779B9) ^ options.variant.wrapping_mul(0x85EBCA6B),
        );
        // Variant zero preserves the documented recipe. Others remain the same character.
        if options.variant != 0 {
            let timing = rng.range(0.96, 1.04);
            let pitch = rng.range(0.98, 1.02);
            for call in &mut calls {
                call.start *= timing;
                call.duration *= timing;
                call.register *= pitch;
            }
        }
        let rate = f64::from(options.sample_rate);
        let duration = calls
            .iter()
            .map(|c| c.start + c.duration)
            .fold(0.0, f64::max)
            + 0.04;
        let mut samples = vec![0.0_f64; (duration * rate).ceil() as usize];
        for call in calls {
            let start = (call.start * rate).round() as usize;
            let part = self
                .voice
                .render(&call, options.sample_rate, strength, &mut rng);
            for (target, source) in samples[start..].iter_mut().zip(part) {
                *target += source;
            }
        }
        let peak = samples
            .iter()
            .fold(1e-12_f64, |peak, value| peak.max(value.abs()));
        // Normalize BEFORE applying user gain, so WAV export preserves volume and intensity.
        let gain =
            10.0_f64.powf(-3.0 / 20.0) * f64::from(options.volume) * (0.35 + 0.65 * strength)
                / peak;
        Ok(Audio {
            samples: samples.into_iter().map(|x| (x * gain) as f32).collect(),
            sample_rate: options.sample_rate,
        })
    }
}
