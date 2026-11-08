use crate::model::Expression;
use clap::ValueEnum;
use mooduck_quacklib::Emotion;
use std::time::Duration;

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum Mode {
    Listen,
    Support,
    #[default]
    Mirror,
}

pub struct Reaction {
    pub emotion: Emotion,
    pub intensity: f32,
    pub volume: f32,
}

/// Small response history, containing no audio or model predictions.
pub struct Responder {
    mode: Mode,
    sad_streak: u8,
    last_sad: Option<Duration>,
}

impl Responder {
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            sad_streak: 0,
            last_sad: None,
        }
    }

    pub fn choose(
        &mut self,
        expression: Expression,
        volume: f32,
        turn: u32,
        now: Duration,
    ) -> Reaction {
        let arousal = expression.arousal.clamp(0.0, 1.0);
        let valence = expression.valence.clamp(0.0, 1.0);
        let (mut emotion, mut intensity, gain) = match self.mode {
            Mode::Listen => (
                if turn.is_multiple_of(3) {
                    Emotion::Yes
                } else {
                    Emotion::Curious
                },
                0.2 + 0.2 * arousal,
                0.8,
            ),
            // Quiet or neutral speech should usually get a brief acknowledgment.
            // A low model valence alone is not enough to warrant a sad response.
            Mode::Support if valence < 0.25 && arousal < 0.4 => (Emotion::Sad, 0.16, 0.6),
            Mode::Support if valence < 0.4 => (Emotion::Yes, 0.18, 0.6),
            Mode::Support if valence > 0.65 => (Emotion::Happy, 0.2 + 0.3 * arousal, 0.8),
            Mode::Support => (Emotion::Curious, 0.2, 0.7),
            Mode::Mirror => {
                let emotion = if valence < 0.3 {
                    if arousal > 0.6 {
                        Emotion::Angry
                    } else {
                        Emotion::Sad
                    }
                } else if valence > 0.6 {
                    if arousal > 0.75 {
                        Emotion::Excited
                    } else {
                        Emotion::Happy
                    }
                } else {
                    Emotion::Curious
                };
                (emotion, 0.2 + 0.65 * arousal, 1.0)
            }
        };

        if emotion == Emotion::Sad {
            self.sad_streak = self.sad_streak.saturating_add(1);
            let ready = self
                .last_sad
                .is_none_or(|last| now.saturating_sub(last) >= Duration::from_secs(15));
            if self.sad_streak >= 2 && ready {
                self.last_sad = Some(now);
                self.sad_streak = 0;
                intensity = intensity.min(0.2);
            } else {
                emotion = Emotion::Yes;
                intensity = 0.18;
            }
        } else {
            self.sad_streak = 0;
        }
        Reaction {
            emotion,
            intensity,
            volume: volume * gain,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mooduck_quacklib::{Options, Synthesizer};

    fn expression(arousal: f32, valence: f32) -> Expression {
        Expression {
            arousal,
            valence,
            dominance: 0.5,
        }
    }

    #[test]
    fn listening_stays_short_even_for_high_arousal() {
        let mut responder = Responder::new(Mode::Listen);
        for turn in 0..9 {
            let reaction = responder.choose(expression(1.0, 0.1), 0.5, turn, Duration::ZERO);
            assert!(matches!(reaction.emotion, Emotion::Curious | Emotion::Yes));
            assert!(reaction.intensity <= 0.5);
        }
    }

    #[test]
    fn support_calms_anger_while_mirroring_reflects_it() {
        let angry = expression(0.9, 0.1);
        assert_eq!(
            Responder::new(Mode::Support)
                .choose(angry, 0.5, 0, Duration::ZERO)
                .emotion,
            Emotion::Yes
        );
        assert_eq!(
            Responder::new(Mode::Mirror)
                .choose(angry, 0.5, 0, Duration::ZERO)
                .emotion,
            Emotion::Angry
        );
    }

    #[test]
    fn neutral_and_mildly_negative_speech_do_not_make_support_sad() {
        let mut responder = Responder::new(Mode::Support);
        for turn in 0..20 {
            for valence in [0.26, 0.35, 0.5, 0.6] {
                let reaction = responder.choose(
                    expression(0.2, valence),
                    0.5,
                    turn,
                    Duration::from_secs(u64::from(turn) * 2),
                );
                assert!(matches!(reaction.emotion, Emotion::Yes | Emotion::Curious));
            }
        }
        let happy = responder.choose(expression(0.6, 0.8), 0.5, 21, Duration::from_secs(42));
        assert_eq!(happy.emotion, Emotion::Happy);
    }

    #[test]
    fn isolated_sad_estimates_are_acknowledged_without_a_sad_response() {
        for mode in [Mode::Support, Mode::Mirror] {
            let mut responder = Responder::new(mode);
            for turn in 0..20 {
                let valence = if turn % 2 == 0 { 0.1 } else { 0.5 };
                let reaction = responder.choose(
                    expression(0.2, valence),
                    0.5,
                    turn,
                    Duration::from_secs(u64::from(turn) * 2),
                );
                assert_ne!(reaction.emotion, Emotion::Sad);
            }
        }
    }

    #[test]
    fn persistent_sad_estimates_get_infrequent_moderate_coos() {
        let synth = Synthesizer::default();
        for mode in [Mode::Support, Mode::Mirror] {
            let mut responder = Responder::new(mode);
            let mut last_sad = None;
            let mut sad_count = 0;
            for turn in 0..30 {
                let now = Duration::from_secs(u64::from(turn) * 2);
                let reaction = responder.choose(expression(0.2, 0.1), 0.5, turn, now);
                if turn == 0 {
                    assert_eq!(reaction.emotion, Emotion::Yes);
                }
                if reaction.emotion == Emotion::Sad {
                    if let Some(last) = last_sad {
                        assert!(now - last >= Duration::from_secs(15));
                    }
                    last_sad = Some(now);
                    sad_count += 1;
                    let audio = synth
                        .render(
                            reaction.emotion,
                            Options {
                                intensity: reaction.intensity,
                                volume: reaction.volume,
                                variant: turn,
                                ..Options::default()
                            },
                        )
                        .unwrap();
                    assert!((0.5..0.9).contains(&audio.duration_seconds()));
                    assert!(reaction.volume <= 0.5);
                }
            }
            assert!((1..=4).contains(&sad_count));
        }
    }
}
