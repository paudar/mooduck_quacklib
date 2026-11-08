use crate::Emotion;

pub(crate) struct Call {
    pub start: f64,
    pub duration: f64,
    pub register: f64,
    pub contour: [f64; 3],
    pub gain: f64,
    pub roughness: f64,
    pub wobble: f64,
    pub breath: f64,
    pub sustained: bool,
}

impl Call {
    fn new(start: f64, duration: f64, register: f64, contour: [f64; 3]) -> Self {
        Self {
            start,
            duration,
            register,
            contour,
            gain: 1.0,
            roughness: 1.0,
            wobble: 1.0,
            breath: 1.0,
            sustained: false,
        }
    }

    pub fn pitch_at(&self, position: f64) -> f64 {
        let (a, b, t) = if position < 0.3 {
            (self.contour[0], self.contour[1], position / 0.3)
        } else {
            (self.contour[1], self.contour[2], (position - 0.3) / 0.7)
        };
        a + (b - a) * t.clamp(0.0, 1.0)
    }
}

pub(crate) fn recipe(emotion: Emotion, intensity: f64, speed: f64, pitch: f64) -> Vec<Call> {
    use Emotion::*;
    let mut calls = match emotion {
        Happy => (0..repetitions(4, intensity))
            .map(|i| {
                let i = f64::from(i);
                Call::new(i * 0.28, 0.23, 1.0 + i * 0.06, [0.0, 5.0, 1.0])
            })
            .collect(),
        Curious => {
            let mut calls = vec![Call::new(0.0, 0.48, 0.85, [-3.0, 0.0, 7.0])];
            // A small question is one short chirp. A second question gradually
            // emerges above half strength, with no long silent tail at the threshold.
            if intensity > 0.5 {
                let follow_up = (intensity - 0.5) * 2.0;
                let mut call = Call::new(
                    0.48 + 0.24 * follow_up,
                    0.58 * follow_up,
                    1.05,
                    [-2.0, 2.0, 9.0],
                );
                call.gain = follow_up * follow_up * (3.0 - 2.0 * follow_up);
                calls.push(call);
            }
            calls
        }
        Sad => {
            // Documented Microduck coo: 200 -> 140 Hz, at full intensity.
            let mut coo = Call::new(0.0, 2.5, 200.0 / pitch, [0.0, -1.7, -6.175]);
            coo.sustained = true;
            coo.roughness = 0.3;
            coo.wobble = 1.6;
            coo.breath = 0.65;
            vec![coo]
        }
        Angry => (0..repetitions(4, intensity))
            .map(|i| {
                let mut call = Call::new(f64::from(i) * 0.29, 0.22, 0.8, [3.0, 1.0, -5.0]);
                call.roughness = 2.0 + intensity;
                call.breath = 1.5;
                call
            })
            .collect(),
        Surprised => vec![Call::new(0.0, 0.52, 1.15, [-2.0, 12.0, 3.0])],
        Yes => vec![Call::new(0.0, 0.36, 1.0, [0.0, -0.7, -3.0])],
        No => vec![
            Call::new(0.0, 0.28, 1.0, [1.0, 0.0, -1.0]),
            Call::new(0.34, 0.42, 1.0, [-4.0, -5.0, -6.0]),
        ],
        Excited => {
            let mut start = 0.0;
            (0..repetitions(6, intensity))
                .map(|i| {
                    let i = f64::from(i);
                    let duration = 0.28 - i * 0.028;
                    let call = Call::new(
                        start,
                        duration,
                        1.0,
                        [i * 1.5, i * 1.5 + 4.0, i * 1.5 + 1.0],
                    );
                    start += duration + 0.09 - i * 0.012;
                    call
                })
                .collect()
        }
        Laugh => {
            let mut calls = vec![Call::new(0.0, 0.5, 1.0, [3.0, 2.0, -1.0])];
            for i in 0..(7.0 * intensity).floor() as u32 {
                let i = f64::from(i);
                let mut call = Call::new(0.58 + i * 0.19, 0.14, 1.0, [2.0, 0.0, -2.0]);
                call.gain = 0.95 - i * 0.09;
                calls.push(call);
            }
            calls
        }
        Confused => {
            let mut call = Call::new(0.0, 0.85, 0.72, [-2.0, -3.0, 5.0]);
            call.sustained = true;
            call.roughness = 0.2;
            call.breath = 0.35;
            vec![call]
        }
    };
    // Scale the phrase length instead of slowing subdued expressions down.
    // Preserve the original full-strength timing, including the slower sad coo.
    let duration_scale =
        (0.35 + 0.65 * intensity) / (if emotion == Sad { 1.0 } else { 1.2 } * speed);
    for call in &mut calls {
        call.start *= duration_scale;
        call.duration *= duration_scale;
        call.roughness *= 0.55 + 0.45 * intensity;
    }
    calls
}

fn repetitions(maximum: u32, intensity: f64) -> u32 {
    1 + (f64::from(maximum - 1) * intensity).floor() as u32
}
