use mooduck_quacklib::{Emotion, Options, Synthesizer};

fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0_f32, |peak, value| peak.max(value.abs()))
}

#[test]
fn every_emotion_is_finite_bounded_and_fades_to_silence_at_supported_rates() {
    let synth = Synthesizer::default();
    for sample_rate in [8_000, 16_000, 44_100, 48_000, 192_000] {
        for emotion in Emotion::ALL {
            for intensity in [0.0, 1.0] {
                let audio = synth
                    .render(
                        emotion,
                        Options {
                            sample_rate,
                            intensity,
                            volume: 1.0,
                            ..Options::default()
                        },
                    )
                    .unwrap();
                assert_eq!(audio.sample_rate(), sample_rate);
                assert!((0.1..5.0).contains(&audio.duration_seconds()));
                assert!(audio.samples().iter().all(|s| s.is_finite()));
                assert!(peak(audio.samples()) > 0.1);
                assert!(peak(audio.samples()) <= 0.708);
                assert_eq!(audio.samples()[0], 0.0);
                assert!(audio.samples().iter().rev().take(100).all(|s| *s == 0.0));
            }
        }
    }
}

#[test]
fn output_volume_is_linear_and_zero_is_silent() {
    let synth = Synthesizer::default();
    let render = |volume| {
        synth
            .render(
                Emotion::Happy,
                Options {
                    volume,
                    ..Options::default()
                },
            )
            .unwrap()
    };
    let full = render(1.0);
    let half = render(0.5);
    let zero = render(0.0);
    assert!(
        full.samples()
            .iter()
            .zip(half.samples())
            .all(|(a, b)| *b == *a * 0.5)
    );
    assert!(zero.samples().iter().all(|s| *s == 0.0));
}

#[test]
fn intensity_changes_expression_not_just_gain() {
    let synth = Synthesizer::default();
    for emotion in Emotion::ALL {
        let low = synth
            .render(
                emotion,
                Options {
                    intensity: 0.2,
                    ..Options::default()
                },
            )
            .unwrap();
        let high = synth
            .render(
                emotion,
                Options {
                    intensity: 1.0,
                    ..Options::default()
                },
            )
            .unwrap();
        assert!(peak(high.samples()) > peak(low.samples()) * 1.5);
        assert!(
            high.duration_seconds() > low.duration_seconds(),
            "{emotion}"
        );
    }
}

#[test]
fn low_curiosity_is_one_short_call_and_high_curiosity_has_two() {
    fn audible_calls(samples: &[f32]) -> usize {
        let threshold = peak(samples) * 0.02;
        let mut active = false;
        let mut count = 0;
        // Count audible regions in 5 ms windows, ignoring individual waveform cycles.
        for window in samples.chunks(240) {
            let rms = (window.iter().map(|s| s * s).sum::<f32>() / window.len() as f32).sqrt();
            let audible = rms > threshold;
            if audible && !active {
                count += 1;
            }
            active = audible;
        }
        count
    }
    let synth = Synthesizer::default();
    for intensity in [0.0, 0.2, 0.5, 0.8, 1.0] {
        let audio = synth
            .render(
                Emotion::Curious,
                Options {
                    intensity,
                    ..Options::default()
                },
            )
            .unwrap();
        assert_eq!(
            audible_calls(audio.samples()),
            if intensity <= 0.5 { 1 } else { 2 },
            "intensity {intensity}"
        );
        if intensity == 0.2 {
            assert!(audio.duration_seconds() < 0.3);
        }
    }
}

#[test]
fn curiosity_duration_grows_smoothly_when_the_second_call_emerges() {
    let synth = Synthesizer::default();
    let mut previous = None;
    for step in 0..=100 {
        let intensity = step as f32 / 100.0;
        let audio = synth
            .render(
                Emotion::Curious,
                Options {
                    intensity,
                    sample_rate: 8_000,
                    ..Options::default()
                },
            )
            .unwrap();
        let duration = audio.duration_seconds();
        if let Some(previous) = previous {
            assert!(duration >= previous);
            assert!(duration - previous < 0.03);
        }
        previous = Some(duration);
    }
}

#[test]
fn rendered_pitch_rises_for_curiosity_and_falls_for_sadness() {
    // Measure actual PCM via normalized autocorrelation, independently of the recipe.
    fn fundamental(samples: &[f32], start_seconds: f64) -> f64 {
        let start = (start_seconds * 48_000.0) as usize;
        let segment: Vec<f64> = samples[start..start + 4800]
            .iter()
            .step_by(4)
            .map(|s| f64::from(*s))
            .collect();
        let (lag, _) = (30..120)
            .map(|lag| {
                let mut dot = 0.0;
                let mut left = 0.0;
                let mut right = 0.0;
                for (a, b) in segment.iter().zip(&segment[lag..]) {
                    dot += a * b;
                    left += a * a;
                    right += b * b;
                }
                (lag, dot / (left * right).sqrt())
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        12_000.0 / lag as f64
    }
    let synth = Synthesizer::default();
    let options = Options {
        intensity: 1.0,
        ..Options::default()
    };
    let curious = synth.render(Emotion::Curious, options).unwrap();
    let sad = synth.render(Emotion::Sad, options).unwrap();
    let curious_start = fundamental(curious.samples(), 0.04);
    let curious_end = fundamental(curious.samples(), 0.31);
    let sad_start = fundamental(sad.samples(), 0.08);
    let sad_end = fundamental(sad.samples(), 2.35);
    assert!(
        curious_end > curious_start * 1.2,
        "{curious_start} -> {curious_end}"
    );
    assert!(sad_end < sad_start * 0.85, "{sad_start} -> {sad_end}");
}

#[test]
fn same_voice_and_variant_repeat_but_other_seeds_and_variants_differ() {
    let synth = Synthesizer::default();
    let first = synth.render(Emotion::Curious, Options::default()).unwrap();
    let repeat = synth.render(Emotion::Curious, Options::default()).unwrap();
    assert_eq!(first.samples(), repeat.samples());
    let other = synth
        .render(
            Emotion::Curious,
            Options {
                variant: 1,
                ..Options::default()
            },
        )
        .unwrap();
    assert_ne!(first.samples(), other.samples());
    let other_voice = Synthesizer::new(12)
        .render(Emotion::Curious, Options::default())
        .unwrap();
    assert_ne!(first.samples(), other_voice.samples());
}

#[test]
fn rejects_invalid_configuration_including_nan_and_infinity() {
    let synth = Synthesizer::default();
    for value in [-0.1, 1.1, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(
            synth
                .render(
                    Emotion::Sad,
                    Options {
                        intensity: value,
                        ..Options::default()
                    }
                )
                .is_err()
        );
        assert!(
            synth
                .render(
                    Emotion::Sad,
                    Options {
                        volume: value,
                        ..Options::default()
                    }
                )
                .is_err()
        );
    }
    for sample_rate in [0, 7_999, 192_001, u32::MAX] {
        assert!(
            synth
                .render(
                    Emotion::Sad,
                    Options {
                        sample_rate,
                        ..Options::default()
                    }
                )
                .is_err()
        );
    }
}

#[test]
fn wav_header_lengths_pcm_and_gain_are_preserved() {
    let synth = Synthesizer::default();
    for volume in [0.0, 0.3] {
        let audio = synth
            .render(
                Emotion::Yes,
                Options {
                    volume,
                    sample_rate: 16_000,
                    ..Options::default()
                },
            )
            .unwrap();
        let mut wav = Vec::new();
        audio.write_wav(&mut wav).unwrap();
        let u32_at = |i| u32::from_le_bytes(wav[i..i + 4].try_into().unwrap());
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..16], b"WAVEfmt ");
        assert_eq!(&wav[20..24], &[1, 0, 1, 0]);
        assert_eq!(u32_at(24), 16_000);
        assert_eq!(u32_at(28), 32_000);
        assert_eq!(u32_at(4) as usize, wav.len() - 8);
        assert_eq!(u32_at(40) as usize, audio.samples().len() * 2);
        let pcm: Vec<_> = wav[44..]
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        for (float, int) in audio.samples().iter().zip(pcm) {
            assert!((float - f32::from(int) / 32767.0).abs() <= 0.51 / 32767.0);
        }
    }
}

#[test]
fn wav_writer_propagates_io_failure() {
    struct Broken;
    impl std::io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("disk full"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let audio = Synthesizer::default()
        .render(Emotion::Yes, Options::default())
        .unwrap();
    assert!(audio.write_wav(Broken).is_err());
}
