# mooduck_quacklib 🦆

A small Rust crate for expressive, synthetic duck sounds inspired by Microduck.
Runs entirely offline, with no external Rust dependencies, bundled audio files,
or AI model. Includes a CLI for listening on a Mac and a cross-platform PCM
interface for use on a robot.

## Try it on a Mac

Requires Rust 1.85 or later. From the project directory:

```sh
cargo run --release -- play happy --intensity 0.8
cargo run --release -- play curious --intensity 0.9
cargo run --release -- play sad --intensity 1.0 --volume 0.4
```

`play` uses `/usr/bin/afplay`, which ships with macOS, and removes the temporary
WAV file afterward. To keep the file, add `--output audio/curious.wav`.
No GUI or additional audio driver is required.

Optionally, install the CLI locally:

```sh
cargo install --path . --offline
mooduck_quacklib play curious --intensity 0.8
```

List all emotions or export WAV files:

```sh
cargo run --release -- list
cargo run --release -- render curious --intensity 0.8 --output audio/curious.wav
cargo run --release -- demo --output-dir audio/demo
cargo run --release --example compare
```

`demo` exports all ten emotions. `compare` renders happiness, curiosity, and
sadness at intensities of 0.2, 0.6, and 1.0 into `audio/comparison/`.
For example, compare:

```sh
afplay audio/comparison/curious-0.2.wav
afplay audio/comparison/curious-1.0.wav
```

## Expression and voice

| Parameter | Meaning | Default |
| --- | --- | --- |
| `--intensity` | 0–1: emotional strength; affects pitch movement, duration, call count, roughness, and level | 0.7 |
| `--volume` | 0–1: additional linear volume multiplier | 0.6 |
| `--seed` | `u32`: stable voice identity, for example one duck voice per robot | 4145077059 |
| `--variant` | `u32`: small, reproducible variation in pitch, timing, and noise | 0 |
| `--sample-rate` | 8000–192000 Hz for PCM and WAV output | 48000 |

Intensity and volume can be adjusted independently: a deeply sad duck can still
be quiet. Low intensity produces short, simple expressions. Higher intensity
lengthens the expression and raises its level; happiness, anger, excitement, and
laughter also gain repetitions. Curiosity consists of one call at intensities
up to and including 0.5; above that, a second call gradually emerges. With the
default voice, curiosity lasts about 0.25 seconds at 0.2 and 1.25 seconds at 1.0.
Sadness ranges from a short coo to a long sigh.
`intensity = 0` produces a subdued expression; `volume = 0` produces silence.
The peak level never exceeds −3 dBFS, even with both controls set to 1.
WAV export preserves the configured output level.

English emotion names and the following German aliases are accepted:

| Emotion | German aliases | Sound |
| --- | --- | --- |
| `happy` | `freude`, `froh` | One to four bouncy quacks with a slight rise in pitch |
| `curious` | `neugier`, `neugierig` | One short call to two longer, questioning calls with rising pitch |
| `sad` | `trauer`, `traurig` | A soft coo with a downward pitch glide, growing longer with intensity |
| `angry` | `wut`, `wütend`, `wuetend` | One to four short, rough barks |
| `surprised` | `überrascht`, `ueberrascht` | A single call with a sharp upward pitch jump |
| `yes` | `ja` | A single quack with falling pitch |
| `no` | `nein` | Two syllables, with the second lower than the first |
| `excited` | `aufgeregt` | One to six increasingly high and short quacks |
| `laugh` | `lachen` | One call followed by up to seven short, fading calls |
| `confused` | `verwirrt`, `mmh` | A muffled, rising “mmh?” |

## Use it in Rust

Add the crate as a local dependency in your application:

```toml
[dependencies]
mooduck_quacklib = { path = "/path/to/mooduck_quacklib" }
```

```rust
use mooduck_quacklib::{Emotion, Options, Synthesizer};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let duck = Synthesizer::default();
    let audio = duck.render(Emotion::Curious, Options {
        intensity: 0.8,
        volume: 0.5,
        sample_rate: 48_000,
        variant: 0,
    })?;

    audio.save_wav("curious.wav")?;

    // Pass mono f32 PCM in the range -1..1 to the robot's audio output:
    let sample_rate = audio.sample_rate();
    let samples = audio.into_samples();
    println!("{} samples at {} Hz", samples.len(), sample_rate);
    Ok(())
}
```

`Synthesizer::new(seed)` creates a different voice. The same seed, emotion, and
options produce the same samples with the same implementation on the same
platform. Floating-point functions may cause small differences between platforms.

### Using it on the HULKs robot

The library core uses only Rust's standard library and does not open audio
devices. It provides mono `f32` samples or 16-bit PCM WAV files. Choose a sample
rate that matches the audio output; for stereo, copy the same samples to both
channels.

Synthesis computes and allocates a complete short clip. Run it in a worker or
during preparation, outside the time-critical audio callback or control loop.
Frequently used sounds can be rendered in advance. Integration with the HULKs
audio stack is not included yet, and the robot hardware has not been tested.
Direct CLI playback currently supports macOS; WAV export and the library are
cross-platform.

## Relationship to Microduck

The voice parameters, random number generator, and weights of the seven harmonics
are ported to Rust from the published Apache-2.0 reference,
[`tools/quack.py`](https://huggingface.co/datasets/pollen-robotics/microduck-emotions/blob/main/tools/quack.py).
The default seed comes from the original demonstrations. Vibrato, slight pitch
jitter, amplitude modulation, and filtered noise form the shared voice design.

The emotion sequences are independent implementations based on the
[documented Microduck expressions](https://huggingface.co/datasets/pollen-robotics/microduck-emotions),
with happiness and surprise added. Noise generation, envelopes, and sample rate
handling differ from the Python reference, so the output does not reproduce the
recordings exactly. Listening is needed to judge the perceived similarity and
how clearly the emotions come across without the robot's movements.

License: Apache-2.0. See [NOTICE](NOTICE) for attribution and a description of changes.

## Development checks

```sh
cargo test --offline
cargo clippy --offline --all-targets -- -D warnings
cargo fmt --check
```

Tests check voice parameters against the Python reference, output bounds and
silence at clip endings, all emotions at multiple sample rates, volume and
intensity, reproducible variants, invalid parameters, and encoded WAV samples.
