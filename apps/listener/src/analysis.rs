use std::{collections::VecDeque, time::Duration};

pub const FRAME_SAMPLES: usize = 160; // 10 ms, mono 16 kHz

/// A bounded utterance tail. No audio leaves memory through this component.
pub struct Segmenter {
    samples: VecDeque<f32>,
    pending_silence: Vec<f32>,
    capacity: usize,
    pause_frames: usize,
    silence_frames: usize,
    speech_frames: usize,
    active: bool,
    last_reaction: Option<Duration>,
    cooldown: Duration,
}

impl Segmenter {
    pub fn new(max_seconds: usize, pause_ms: u64, cooldown_ms: u64) -> Self {
        Self {
            samples: VecDeque::with_capacity(max_seconds * 16_000),
            pending_silence: Vec::with_capacity(pause_ms.div_ceil(10) as usize * FRAME_SAMPLES),
            capacity: max_seconds * 16_000,
            pause_frames: pause_ms.div_ceil(10) as usize,
            silence_frames: 0,
            speech_frames: 0,
            active: false,
            last_reaction: None,
            cooldown: Duration::from_millis(cooldown_ms),
        }
    }

    pub fn clear(&mut self) {
        let (a, b) = self.samples.as_mut_slices();
        a.fill(0.0);
        b.fill(0.0);
        self.samples.clear();
        self.pending_silence.fill(0.0);
        self.pending_silence.clear();
        self.silence_frames = 0;
        self.speech_frames = 0;
        self.active = false;
    }

    pub fn mark_reaction(&mut self, now: Duration) {
        self.last_reaction = Some(now);
    }

    pub fn feed(
        &mut self,
        frame: &[f32; FRAME_SAMPLES],
        speech: bool,
        now: Duration,
    ) -> Option<Vec<f32>> {
        if speech {
            self.active = true;
            self.speech_frames = self.speech_frames.saturating_add(1);
            self.silence_frames = 0;
        } else if self.active {
            self.silence_frames += 1;
        } else {
            return None;
        }

        // Preserve short internal pauses, but do not include the final pause in
        // the model input. Both buffers have limits independent of session length.
        if speech {
            for sample in self.pending_silence.drain(..) {
                if self.samples.len() == self.capacity {
                    self.samples.pop_front();
                }
                self.samples.push_back(sample);
            }
            for sample in frame {
                if self.samples.len() == self.capacity {
                    self.samples.pop_front();
                }
                self.samples.push_back(*sample);
            }
        } else {
            self.pending_silence.extend_from_slice(frame);
        }
        if self.silence_frames < self.pause_frames {
            return None;
        }
        let enough_speech = self.speech_frames >= 30; // Ignore isolated clicks / very short fragments.
        let ready = self
            .last_reaction
            .is_none_or(|last| now.saturating_sub(last) >= self.cooldown);
        let output = if enough_speech && ready {
            Some(self.samples.iter().copied().collect())
        } else {
            None
        };
        self.clear(); // Never respond later to an utterance suppressed by the cooldown.
        output
    }
}

impl Drop for Segmenter {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn feed(
        segmenter: &mut Segmenter,
        count: u64,
        speech: bool,
        start_ms: u64,
    ) -> Option<Vec<f32>> {
        let mut result = None;
        for i in 0..count {
            let next = segmenter.feed(
                &[0.2; FRAME_SAMPLES],
                speech,
                Duration::from_millis(start_ms + i * 10),
            );
            if next.is_some() {
                assert!(result.is_none());
                result = next;
            }
        }
        result
    }

    #[test]
    fn waits_for_pause_and_emits_only_once() {
        let mut s = Segmenter::new(3, 500, 3000);
        assert!(feed(&mut s, 50, true, 0).is_none());
        assert!(feed(&mut s, 49, false, 500).is_none());
        assert!(feed(&mut s, 1, false, 990).is_some());
        assert!(feed(&mut s, 100, false, 1000).is_none());
    }

    #[test]
    fn shorter_pause_and_two_second_cooldown_observe_the_boundary() {
        for end_ms in [1990, 2000] {
            let mut s = Segmenter::new(3, 250, 2000);
            s.mark_reaction(Duration::ZERO);
            feed(&mut s, 30, true, end_ms - 540);
            assert!(feed(&mut s, 24, false, end_ms - 240).is_none());
            let clip = feed(&mut s, 1, false, end_ms);
            assert_eq!(clip.is_some(), end_ms == 2000);
            assert!(feed(&mut s, 100, false, end_ms + 10).is_none());
        }
    }

    #[test]
    fn cooldown_drops_old_utterances_instead_of_queuing_them() {
        let mut s = Segmenter::new(3, 500, 3000);
        s.mark_reaction(Duration::ZERO);
        feed(&mut s, 50, true, 1000);
        assert!(feed(&mut s, 50, false, 1500).is_none());
        assert!(feed(&mut s, 400, false, 2000).is_none());
        feed(&mut s, 50, true, 6000);
        assert!(feed(&mut s, 50, false, 6500).is_some());
    }

    #[test]
    fn memory_is_bounded_and_short_noises_do_not_trigger() {
        let mut s = Segmenter::new(3, 500, 3000);
        feed(&mut s, 2000, true, 0);
        assert_eq!(s.samples.len(), 48_000);
        assert_eq!(feed(&mut s, 50, false, 20_000).unwrap().len(), 48_000);
        assert!(s.samples.is_empty());
        feed(&mut s, 5, true, 25_000);
        assert!(feed(&mut s, 50, false, 25_050).is_none());
    }

    #[test]
    fn resumed_speech_restarts_the_pause_timer() {
        let mut s = Segmenter::new(3, 500, 3000);
        feed(&mut s, 50, true, 0);
        assert!(feed(&mut s, 40, false, 500).is_none());
        feed(&mut s, 10, true, 900);
        assert!(feed(&mut s, 49, false, 1000).is_none());
        assert!(feed(&mut s, 1, false, 1490).is_some());
    }

    #[test]
    fn preserves_short_internal_pauses_but_trims_the_final_pause() {
        let mut s = Segmenter::new(3, 500, 3000);
        feed(&mut s, 40, true, 0);
        for i in 0..20 {
            assert!(
                s.feed(
                    &[0.0; FRAME_SAMPLES],
                    false,
                    Duration::from_millis(400 + i * 10)
                )
                .is_none()
            );
        }
        feed(&mut s, 40, true, 600);
        let clip = feed(&mut s, 50, false, 1000).unwrap();
        assert_eq!(clip.len(), 100 * FRAME_SAMPLES);
        assert!(
            clip[40 * FRAME_SAMPLES..60 * FRAME_SAMPLES]
                .iter()
                .all(|v| *v == 0.0)
        );
        assert_eq!(s.pending_silence.len(), 0);
    }
}
