//! Wav2Small 2.0 CPU inference. Architecture and model provenance: see MODEL.md.
//! Model weights are separate from this program and have their own license.
use anyhow::{Context, Result, bail};
use candle_core::{DType, Device, Tensor};
use candle_nn::{
    BatchNorm, Conv1d, Conv1dConfig, Conv2d, Conv2dConfig, Linear, Module, ModuleT, VarBuilder,
};
use sha2::{Digest, Sha256};
use std::{path::Path, time::Duration};

pub const REVISION: &str = "6ea3fe5be073ba13083bb60d14af2736dc258de9";
pub const WEIGHTS_SHA256: &str = "86ccef83ce942ff34299a44684e9b69bf37021dd4b69325c0dcb3decb0df2f4d";
pub const SAMPLE_RATE: usize = 16_000;

#[derive(Clone, Copy, Debug)]
pub struct Expression {
    pub arousal: f32,
    pub dominance: f32,
    pub valence: f32,
}

struct Block {
    conv: Conv2d,
    norm: BatchNorm,
}

pub struct EmotionModel {
    real: Conv1d,
    imag: Conv1d,
    mel: Tensor,
    blocks: Vec<Block>,
    values: Conv2d,
    attention: Conv2d,
    head: Linear,
}

pub fn verify_weights(bytes: &[u8]) -> Result<()> {
    let digest = format!("{:x}", Sha256::digest(bytes));
    if digest != WEIGHTS_SHA256 {
        bail!(
            "model checksum mismatch; run setup-model again or use the documented pinned weights"
        );
    }
    Ok(())
}

impl EmotionModel {
    /// Read verified local weights. This path never performs a network request.
    pub fn load(directory: &Path) -> Result<Self> {
        let path = directory.join("model.safetensors");
        let bytes = std::fs::read(&path).with_context(|| {
            format!(
                "cannot read {}; run `mooduck_listener setup-model` first",
                path.display()
            )
        })?;
        verify_weights(&bytes)?;
        let device = Device::Cpu;
        let tensors = candle_core::safetensors::load_buffer(&bytes, &device)?;
        let vb = VarBuilder::from_tensors(tensors, DType::F32, &device);
        let vgg = vb.pp("vgg7");
        let spectral = vgg.pp("spectrogram_extractor");
        let spectral_config = Conv1dConfig {
            stride: 32,
            ..Default::default()
        };
        let real = candle_nn::conv1d_no_bias(1, 33, 64, spectral_config, spectral.pp("conv_real"))?;
        let imag = candle_nn::conv1d_no_bias(1, 33, 64, spectral_config, spectral.pp("conv_imag"))?;
        let mel = vgg.pp("logmel_extractor").get((33, 26), "melW")?;
        let mut blocks = Vec::new();
        for i in 1..=7 {
            let layer = vgg.pp(format!("l{i}"));
            blocks.push(Block {
                conv: candle_nn::conv2d_no_bias(
                    if i == 1 { 1 } else { 13 },
                    13,
                    3,
                    Conv2dConfig {
                        padding: 1,
                        ..Default::default()
                    },
                    layer.pp("conv"),
                )?,
                norm: candle_nn::batch_norm(13, 1e-5, layer.pp("norm"))?,
            });
        }
        Ok(Self {
            real,
            imag,
            mel,
            blocks,
            values: candle_nn::conv2d(13, 13, 1, Default::default(), vgg.pp("lin"))?,
            attention: candle_nn::conv2d(13, 13, 1, Default::default(), vgg.pp("sof"))?,
            head: candle_nn::linear(338, 3, vb.pp("adv"))?,
        })
    }

    /// Predict continuous acoustic expression, not emotion probabilities or a transcript.
    pub fn predict(&self, samples: &[f32]) -> Result<Expression> {
        if !(SAMPLE_RATE / 5..=SAMPLE_RATE * 10).contains(&samples.len())
            || samples.iter().any(|v| !v.is_finite())
        {
            bail!("emotion input must contain 0.2–10 seconds of finite 16 kHz mono samples");
        }
        // Match the published population-variance normalization (epsilon 1e-7).
        let mean = samples.iter().sum::<f32>() / samples.len() as f32;
        let variance =
            samples.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / samples.len() as f32;
        let normalized: Vec<_> = samples
            .iter()
            .map(|v| (v - mean) / (variance + 1e-7).sqrt())
            .collect();
        let x = Tensor::from_vec(normalized, (1, 1, samples.len()), &Device::Cpu)?;
        let power = (self.real.forward(&x)?.sqr()? + self.imag.forward(&x)?.sqr()?)?;
        // [batch, frequency, time] -> [time, frequency] -> log-mel [1, 1, time, mel].
        let x = power
            .squeeze(0)?
            .transpose(0, 1)?
            .contiguous()?
            .matmul(&self.mel)?;
        let x = (x.clamp(1e-10_f32, f32::MAX)?.log()? * (10.0 / std::f64::consts::LN_10))?;
        let mut x = x.unsqueeze(0)?.unsqueeze(0)?;
        for (i, block) in self.blocks.iter().enumerate() {
            x = block
                .norm
                .forward_t(&block.conv.forward(&x)?, false)?
                .relu()?;
            if i == 2 {
                // PyTorch max_pool2d(kernel=3, stride=2, padding=1). ReLU makes
                // zero-padding equivalent to -infinity for the maximum.
                x = x
                    .pad_with_zeros(2, 1, 1)?
                    .pad_with_zeros(3, 1, 1)?
                    .max_pool2d_with_stride(3, 2)?;
            }
        }
        let weights = candle_nn::ops::softmax(&self.attention.forward(&x)?, 2)?;
        let pooled = self.values.forward(&x)?.mul(&weights)?.sum(2)?;
        let gram = pooled.matmul(&pooled.transpose(1, 2)?.contiguous()?)?;
        let features = Tensor::cat(&[&pooled, &gram], 2)?.reshape((1, 338))?;
        let values = self
            .head
            .forward(&features)?
            .flatten_all()?
            .to_vec1::<f32>()?;
        if values.iter().any(|v| !v.is_finite()) {
            bail!("emotion model produced non-finite values");
        }
        Ok(Expression {
            arousal: values[0],
            dominance: values[1],
            valence: values[2],
        })
    }
}

/// The only network entry point. Called exclusively by the explicit setup command.
pub fn setup(directory: &Path) -> Result<()> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .into();
    let mut files = Vec::new();
    for (remote, local) in [
        ("model.safetensors", "model.safetensors"),
        ("LICENSE", "LICENSE"),
        ("README.md", "MODEL.md"),
    ] {
        println!("Downloading audeering/wav2small {remote} ({REVISION})");
        let url = format!("https://huggingface.co/audeering/wav2small/resolve/{REVISION}/{remote}");
        let bytes = agent
            .get(&url)
            .call()?
            .body_mut()
            .with_config()
            .limit(1_048_576)
            .read_to_vec()?;
        if local == "model.safetensors" {
            verify_weights(&bytes)?;
        }
        files.push((local, bytes));
    }
    std::fs::create_dir_all(directory)?;
    for (name, bytes) in files {
        let partial = directory.join(format!(".{name}.part"));
        std::fs::write(&partial, bytes)?;
        std::fs::rename(&partial, directory.join(name))?;
    }
    println!(
        "Ready: {}. Model license: CC-BY-NC-SA-4.0; upstream specifies research use.",
        directory.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unverified_model_bytes() {
        assert!(verify_weights(b"not a model").is_err());
    }

    #[test]
    #[ignore = "requires explicit setup-model; never downloads weights during tests"]
    fn pinned_model_matches_reference() {
        let directory = std::env::var_os("MOODUCK_MODEL_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../models/wav2small")
            });
        let model = EmotionModel::load(&directory).unwrap();
        // Golden outputs from the published architecture evaluated with PyTorch
        // and the pinned safetensors weights. No recordings are used or bundled.
        for (length, expected) in [
            (16_000, [0.26208663, 0.28793496, -0.09424567]),
            (48_000, [0.27476102, 0.29590148, -0.09091771]),
        ] {
            let samples: Vec<_> = (0..length)
                .map(|i| ((i % 251) as f32 / 125.0 - 1.0) * 0.2)
                .collect();
            let value = model.predict(&samples).unwrap();
            for (actual, expected) in [value.arousal, value.dominance, value.valence]
                .into_iter()
                .zip(expected)
            {
                assert!(
                    (actual - expected).abs() < 2e-4,
                    "{length} samples: {actual} != {expected}"
                );
            }
        }
        assert!(model.predict(&[0.0; 32]).is_err());
        assert!(model.predict(&vec![f32::NAN; 16_000]).is_err());
    }
}
