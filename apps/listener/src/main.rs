mod analysis;
mod audio;
mod model;
mod reaction;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use mooduck_quacklib::{Options, Synthesizer};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use webrtc_vad::{SampleRate, Vad, VadMode};

#[derive(Parser)]
#[command(
    version,
    about = "Local voice-expression listener. Audio stays in bounded RAM buffers; no transcripts or conversation files."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Explicitly download the pinned model, its license, and model card. No audio access.
    SetupModel(ModelPath),
    /// Validate the local model using a synthetic signal. No microphone access or downloads.
    CheckModel(ModelPath),
    /// List input and output device names without opening microphone streams.
    Devices,
    /// Start listening locally; stop with Ctrl+C. Never downloads or saves conversation audio.
    Run(RunArgs),
}

#[derive(Args)]
struct ModelPath {
    #[arg(long, default_value = "models/wav2small")]
    model_dir: PathBuf,
}

#[derive(Args)]
struct RunArgs {
    #[command(flatten)]
    model: ModelPath,
    #[arg(long, value_enum, default_value = "mirror")]
    mode: reaction::Mode,
    #[arg(long, default_value_t = 250, value_parser = clap::value_parser!(u64).range(100..=3000))]
    pause_ms: u64,
    #[arg(long, default_value_t = 2000, value_parser = clap::value_parser!(u64).range(500..=60_000))]
    cooldown_ms: u64,
    #[arg(long, default_value_t = 200, value_parser = clap::value_parser!(u64).range(0..=2000))]
    tail_ms: u64,
    #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u8).range(1..=10))]
    max_audio_seconds: u8,
    #[arg(long, default_value_t = 0.5, value_parser = volume)]
    volume: f32,
    #[arg(long, default_value_t = mooduck_quacklib::MICRODUCK_SEED)]
    seed: u32,
    /// Exact name from the devices command. Defaults to the system microphone.
    #[arg(long)]
    input_device: Option<String>,
    /// Exact name from the devices command. Defaults to the system output.
    #[arg(long)]
    output_device: Option<String>,
}

fn volume(value: &str) -> std::result::Result<f32, String> {
    let value: f32 = value.parse().map_err(|_| "volume must be a number")?;
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err("volume must be finite and between 0 and 1".into());
    }
    Ok(value)
}

struct SpeechInput {
    vad: Vad,
    resampler: audio::Resampler,
    frame: [f32; analysis::FRAME_SAMPLES],
    filled: usize,
    segmenter: analysis::Segmenter,
}

impl SpeechInput {
    fn new(rate: u32, args: &RunArgs) -> Self {
        Self {
            vad: Vad::new_with_rate_and_mode(SampleRate::Rate16kHz, VadMode::Aggressive),
            resampler: audio::Resampler::new(rate),
            frame: [0.0; analysis::FRAME_SAMPLES],
            filled: 0,
            segmenter: analysis::Segmenter::new(
                usize::from(args.max_audio_seconds),
                args.pause_ms,
                args.cooldown_ms,
            ),
        }
    }

    fn reset(&mut self) {
        self.vad = Vad::new_with_rate_and_mode(SampleRate::Rate16kHz, VadMode::Aggressive);
        self.resampler.reset();
        self.frame.fill(0.0);
        self.filled = 0;
        self.segmenter.clear();
    }

    fn feed(&mut self, sample: f32, now: Duration) -> Result<(bool, Option<Vec<f32>>)> {
        let (converted, count) = self.resampler.push(sample);
        let mut heard_speech = false;
        let mut candidate = None;
        for value in &converted[..count] {
            self.frame[self.filled] = *value;
            self.filled += 1;
            if self.filled == self.frame.len() {
                let pcm = self
                    .frame
                    .map(|s| (s.clamp(-1.0, 1.0) * 32767.0).round() as i16);
                let speech = self
                    .vad
                    .is_voice_segment(&pcm)
                    .map_err(|_| anyhow::anyhow!("voice activity detector rejected a frame"))?;
                heard_speech |= speech;
                candidate = self.segmenter.feed(&self.frame, speech, now).or(candidate);
                self.frame.fill(0.0);
                self.filled = 0;
            }
        }
        Ok((heard_speech, candidate))
    }
}

fn wait(running: &AtomicBool, duration: Duration) {
    let until = Instant::now() + duration;
    while running.load(Ordering::Relaxed) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn listen(args: RunArgs) -> Result<()> {
    let model = model::EmotionModel::load(&args.model.model_dir)?;
    let running = Arc::new(AtomicBool::new(true));
    let signal = running.clone();
    ctrlc::set_handler(move || {
        signal.store(false, Ordering::SeqCst);
    })?;
    let mut speaker = audio::Speaker::open(args.output_device.as_deref())?;
    let mut microphone =
        audio::Microphone::open(args.input_device.as_deref()).context("cannot open microphone")?;
    let mut input = SpeechInput::new(microphone.sample_rate, &args);
    let voice = Synthesizer::new(args.seed);
    let mut responder = reaction::Responder::new(args.mode);
    let start = Instant::now();
    let mut turn = 0_u32;
    println!(
        "Listening locally ({:?}); pause {} ms, cooldown {} ms. Ctrl+C stops. No audio files or conversation logs.",
        args.mode, args.pause_ms, args.cooldown_ms
    );
    while running.load(Ordering::Relaxed) {
        microphone.check()?;
        if microphone.take_dropped() != 0 {
            microphone.discard_pending();
            input.reset();
        }
        let Some(sample) = microphone.pop() else {
            std::thread::sleep(Duration::from_millis(2));
            continue;
        };
        let (_, candidate) = input.feed(sample, start.elapsed())?;
        let Some(mut samples) = candidate else {
            continue;
        };
        let prediction = model.predict(&samples);
        samples.fill(0.0); // Reuse/clear audio promptly; model intermediates are transient RAM tensors.
        let prediction = prediction?;

        // If speech resumed during inference, cancel this response and keep listening.
        let mut resumed = microphone.take_dropped() != 0;
        while let Some(sample) = microphone.pop() {
            let (speech, stale) = input.feed(sample, start.elapsed())?;
            resumed |= speech;
            if let Some(mut stale) = stale {
                stale.fill(0.0);
            }
        }
        if resumed || !running.load(Ordering::Relaxed) {
            continue;
        }

        // Half-duplex by choice: discard microphone audio through playback and its tail.
        microphone.gate(false);
        input.reset();
        let reaction = responder.choose(prediction, args.volume, turn, start.elapsed());
        let audio = voice.render(
            reaction.emotion,
            Options {
                intensity: reaction.intensity,
                volume: reaction.volume,
                sample_rate: speaker.sample_rate,
                variant: turn,
            },
        )?;
        input.segmenter.mark_reaction(start.elapsed());
        speaker.play(audio.samples(), &running)?;
        wait(&running, Duration::from_millis(args.tail_ms));
        microphone.discard_pending();
        input.reset();
        microphone.gate(true);
        turn = turn.wrapping_add(1);
    }
    microphone.gate(false);
    input.reset();
    println!("Stopped.");
    Ok(())
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::SetupModel(path) => model::setup(&path.model_dir),
        Command::Devices => audio::devices(),
        Command::Run(args) => listen(args),
        Command::CheckModel(path) => {
            let model = model::EmotionModel::load(&path.model_dir)?;
            let samples: Vec<_> = (0..16_000)
                .map(|i| 0.2 * (std::f32::consts::TAU * 180.0 * i as f32 / 16_000.0).sin())
                .collect();
            let start = Instant::now();
            let value = model.predict(&samples)?;
            if !value.dominance.is_finite() {
                bail!("invalid model result");
            }
            println!(
                "Local model OK ({:.0} ms on a synthetic 1 s signal).",
                start.elapsed().as_secs_f64() * 1000.0
            );
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_match_the_requested_behavior() {
        let cli = Cli::try_parse_from(["listener", "run"]).unwrap();
        let Command::Run(args) = cli.command else {
            panic!()
        };
        assert!(matches!(args.mode, reaction::Mode::Mirror));
        assert!(matches!(reaction::Mode::default(), reaction::Mode::Mirror));
        assert_eq!(args.pause_ms, 250);
        assert_eq!(args.cooldown_ms, 2000);
        assert_eq!(args.tail_ms, 200);
    }

    #[test]
    fn invalid_volume_and_timings_fail_before_opening_devices() {
        for args in [
            ["listener", "run", "--volume", "NaN"],
            ["listener", "run", "--volume", "1.1"],
            ["listener", "run", "--pause-ms", "0"],
            ["listener", "run", "--max-audio-seconds", "100"],
        ] {
            assert!(Cli::try_parse_from(args).is_err());
        }
    }

    #[test]
    fn silence_does_not_trigger_an_utterance_through_resampling_and_vad() {
        let cli = Cli::try_parse_from(["listener", "run"]).unwrap();
        let Command::Run(args) = cli.command else {
            panic!()
        };
        let mut input = SpeechInput::new(48_000, &args);
        for _ in 0..48_000 {
            let (speech, candidate) = input.feed(0.0, Duration::from_secs(1)).unwrap();
            assert!(!speech);
            assert!(candidate.is_none());
        }
        input.reset();
        assert_eq!(input.filled, 0);
    }
}
