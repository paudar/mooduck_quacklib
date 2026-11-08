# mooduck_listener

A local companion app for `mooduck_quacklib`. It listens to vocal expression,
waits for a speaking pause, and responds with a short synthetic duck sound.
It uses a local emotion model and does not transcribe or interpret your words.

The default is to mirror estimated vocal expression after **250 ms of detected silence**,
with at least **2 seconds between response starts**. While a quack plays and for
200 ms afterward, incoming microphone samples are discarded. Quacks finish
normally; speaking over one does not interrupt it.

## Build and set up

Run these commands from the repository root. The listener requires Rust 1.88 or
later for its model backend; the separate sound crate still supports Rust 1.85.

```sh
cargo build --release -p mooduck_listener
cargo run --release -p mooduck_listener -- setup-model
cargo run --release -p mooduck_listener -- check-model
```

`setup-model` is the **only command that accesses the network**. It downloads
the pinned Wav2Small weights (about 69 KB), their license, and their model card
into `models/wav2small/`, which is ignored by Git. The weights are checked with
SHA-256 during download and every time they are loaded. `check-model` runs a
synthetic signal through the model without opening a microphone.

The model is provided by audEERING under **CC-BY-NC-SA-4.0**, with research use
specified in its model card. It is separate from the Apache-2.0 application and
sound library. See [MODEL.md](MODEL.md) for the exact source and license links.
The model is not bundled in the repository or the Rust crate package.

On macOS, Xcode Command Line Tools provide the C compiler used by the small
WebRTC voice activity detector. Grant microphone access to the terminal or app
that launches the listener in **System Settings → Privacy & Security → Microphone**.

On Debian/Ubuntu Linux, install the build requirements before compiling:

```sh
sudo apt install build-essential pkg-config libasound2-dev
```

The app uses CPAL for direct microphone and speaker access, so `afplay`,
`pw-play`, `paplay`, and `aplay` are not needed for this listener. A working
audio device and the appropriate OS audio configuration are required.

## Run

```sh
# Default: reflect the estimated vocal expression (mirror mode)
cargo run --release -p mooduck_listener -- run

# Softer, supportive reactions
cargo run --release -p mooduck_listener -- run --mode support

# Brief listening acknowledgments
cargo run --release -p mooduck_listener -- run --mode listen
```

Press **Ctrl+C** to stop. An omitted subcommand prints usage and does not start
the microphone. The default input and output devices are used; to select others:

```sh
cargo run --release -p mooduck_listener -- devices
cargo run --release -p mooduck_listener -- run \
  --input-device "Exact input device name" \
  --output-device "Exact output device name"
```

If `devices` reports no input devices, make a microphone or headset available
to the OS. On macOS, first check **System Settings → Sound → Input**: an input
must be available and selected there. Then start `run` from the standalone
Terminal app and allow microphone access if prompted. Terminal normally appears
under **Privacy & Security → Microphone** after it requests access; it cannot be
added manually to that list. Restart the listener after changing permissions.

CPAL 0.16 can report a placeholder default microphone on macOS even when no
input hardware is available. The listener checks for usable input devices first
and reports this separately from failures to configure or start a stream.

| Mode | Behavior |
| --- | --- |
| `listen` | Short curious sounds and occasional agreeing quacks; intensity remains low. |
| `support` | Mostly brief, gentle acknowledgments; restrained happy sounds for clearly positive expression, and occasional short coos for sustained strongly negative/low-energy estimates. |
| `mirror` (default) | Maps estimated activation and valence to sad, angry, curious, happy, or excited sounds; sad responses are brief and rate-limited. |

In `support` and `mirror`, a sad sound requires two consecutive sad response
candidates and at least **15 seconds since the previous sad sound**. Until then,
the duck gives a soft agreeing quack. Support uses stricter thresholds: valence
below 0.25 and arousal below 0.4; moderately negative and neutral estimates get
brief agreeing or curious sounds. Positive estimates above 0.65 get happy sounds.
Sad response intensity is capped at 0.2, giving a coo of roughly 0.6–0.8 seconds
with the default voice. This history stores only a counter and a timestamp.

The model outputs continuous arousal, dominance, and valence estimates. These
are not calibrated emotion probabilities. The modes are hand-designed response
policies based on arousal and valence; they do not establish how someone feels.
German speech and your microphone environment need listening tests and tuning.

### Options

```sh
cargo run --release -p mooduck_listener -- run \
  --pause-ms 250 --cooldown-ms 2000 --tail-ms 200 \
  --volume 0.5 --max-audio-seconds 3
```

| Option | Default | Meaning |
| --- | --- | --- |
| `--mode` | `mirror` | `listen`, `support`, or `mirror` |
| `--pause-ms` | `250` | Required VAD silence, 100–3000 ms |
| `--cooldown-ms` | `2000` | Minimum interval between response starts, 500–60000 ms |
| `--tail-ms` | `200` | Extra microphone suppression after playback, 0–2000 ms |
| `--max-audio-seconds` | `3` | Maximum recent utterance audio retained for inference, 1–10 s |
| `--volume` | `0.5` | Overall output gain, 0–1; response modes can lower it further |
| `--seed` | `4145077059` | Stable duck voice identity |
| `--model-dir` | `models/wav2small` | Local directory containing verified weights |
| `--input-device` | system default | Exact input device name from `devices` |
| `--output-device` | system default | Exact output device name from `devices` |

The pause starts after WebRTC VAD stops detecting speech. VAD hangover and model
inference add latency beyond `--pause-ms`; 250 ms is the configured silence
window, not a measured end-to-end response time. For quicker responses, try
`--pause-ms 150`; increase it if the duck responds during short thinking pauses.
Fragments shorter than 300 ms of detected speech are ignored. Suppressed reactions are dropped rather than queued.
If speech resumes during model inference, the old reaction is canceled.

This first version uses **half-duplex speaker handling**, not acoustic echo
cancellation. It discards what you say during a quack and its configured tail.
Increase `--tail-ms` if room echo causes false reactions. It treats all speech
picked up by the microphone alike; it does not identify individual speakers.

## Privacy and resource use

- `run` loads a local model and makes no network requests. It never downloads a
  missing model automatically.
- No microphone audio, transcripts, embeddings, emotion history, or conversation
  logs are written to disk. Generated duck audio also stays in RAM.
- The microphone handoff holds at most 0.5 s of mono audio. The utterance tail is
  capped by `--max-audio-seconds`; a separate pending-pause buffer is capped by
  `--pause-ms`. Resampling and frame buffers have fixed sizes. Model intermediates
  are temporary RAM tensors. Buffers are cleared after use or while gated.
- The terminal prints startup/shutdown status and operational errors, not
  utterance content or per-utterance emotion estimates.
- This is an application-level storage policy, not a guarantee against OS swap,
  crash dumps, or recording by other software.

Inference runs on the CPU using Candle. No Python, PyTorch installation, GPU,
ONNX Runtime download, or external playback process is needed at runtime.
Native audio callbacks use fixed-capacity lock-free queues; resampling, VAD,
inference, and sound synthesis run outside those callbacks.

## Development

```sh
cargo test --workspace --offline
cargo clippy --workspace --all-targets --offline -- -D warnings
cargo fmt --all --check
```

Tests use synthetic audio and do not open the microphone or download models.
After explicit setup, the optional model parity check can be run with:

```sh
cargo test -p mooduck_listener --offline pinned_model_matches_reference \
  -- --ignored --nocapture
```

The existing sound-only CLI remains the default workspace package:

```sh
cargo run --release -- play curious --intensity 0.2
```
