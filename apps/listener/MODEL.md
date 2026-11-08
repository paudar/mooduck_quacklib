# Model provenance

The listener uses **Wav2Small 2.0** from audEERING. It consumes normalized raw
16 kHz mono audio and predicts arousal, dominance, and valence. The app implements
the published neural network architecture in Rust with Candle, using its original
weights, spectral kernels, mel filter bank, and inference normalization.

- Model: <https://huggingface.co/audeering/wav2small>
- Pinned revision: `6ea3fe5be073ba13083bb60d14af2736dc258de9`
- Weight file: `model.safetensors`, 68,716 bytes
- SHA-256: `86ccef83ce942ff34299a44684e9b69bf37021dd4b69325c0dcb3decb0df2f4d`
- Model card and architecture: <https://huggingface.co/audeering/wav2small/blob/6ea3fe5be073ba13083bb60d14af2736dc258de9/README.md>
- Model license: **CC-BY-NC-SA-4.0**; the model card specifies research use.
- Full model license: <https://huggingface.co/audeering/wav2small/blob/6ea3fe5be073ba13083bb60d14af2736dc258de9/LICENSE>
- Paper: <https://arxiv.org/abs/2408.13920>

Paper authors: Dionyssos Kounades-Bastian, Oliver Schrüfer, Anna Derington,
Hagen Wierstorf, Florian Eyben, Felix Burkhardt, and Björn W. Schuller.

`setup-model` downloads the original weights, license, and model card into the
chosen local model directory. They are not distributed with this repository or
licensed under the sound library's Apache-2.0 license. `run` never downloads them.

Response categories such as `happy` or `sad` are the app's heuristic mapping of
continuous model output, not labels or confidence scores supplied by Wav2Small.
The app does not claim validated recognition accuracy for German or for any
particular speaker, microphone, room, or emotional state.

Voice activity detection uses the separate WebRTC/libfvad implementation via
`webrtc-vad`; it requires no model download. Its role is speech/pause detection,
not expression estimation.
