use anyhow::{Context, Result, bail};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

pub struct Microphone {
    _stream: cpal::Stream,
    consumer: Consumer<f32>,
    enabled: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    dropped: Arc<AtomicUsize>,
    pub sample_rate: u32,
}

pub struct Speaker {
    _stream: cpal::Stream,
    producer: Producer<f32>,
    remaining: Arc<AtomicUsize>,
    failed: Arc<AtomicBool>,
    pub sample_rate: u32,
}

pub fn devices() -> Result<()> {
    let host = cpal::default_host();
    for (kind, devices) in [
        ("input", host.input_devices()?),
        ("output", host.output_devices()?),
    ] {
        let mut found = false;
        for device in devices {
            found = true;
            println!("{kind}: {}", device.name()?);
        }
        if !found {
            println!("{kind}: (no devices available)");
        }
    }
    Ok(())
}

fn device(input: bool, name: Option<&str>) -> Result<cpal::Device> {
    let host = cpal::default_host();
    if let Some(name) = name {
        let mut devices = if input {
            host.input_devices()?
        } else {
            host.output_devices()?
        };
        return devices
            .find(|d| d.name().is_ok_and(|n| n == name))
            .with_context(|| format!("audio device {name:?} not found; use the devices command"));
    }
    // CPAL 0.16 can return a placeholder default device on macOS even when
    // there is no input hardware. Query usable inputs before asking for its config.
    if input
        && host
            .input_devices()
            .context("cannot enumerate microphone devices")?
            .next()
            .is_none()
    {
        bail!(
            "no usable microphone devices reported; connect or enable a microphone, \
             check System Settings > Sound > Input on macOS, then use the devices command. \
             If an input is listed in system settings, check microphone permissions and audio drivers"
        );
    }
    (if input {
        host.default_input_device()
    } else {
        host.default_output_device()
    })
    .with_context(|| {
        format!(
            "no default {} device available",
            if input { "microphone" } else { "output" }
        )
    })
}

fn input_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut producer: Producer<f32>,
    enabled: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    dropped: Arc<AtomicUsize>,
) -> Result<cpal::Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = usize::from(config.channels);
    Ok(device.build_input_stream(
        config,
        move |data: &[T], _| {
            // Fixed-capacity, lock-free handoff; no allocation, disk I/O, or inference here.
            for frame in data.chunks_exact(channels) {
                if !enabled.load(Ordering::Relaxed) {
                    continue;
                }
                let mono =
                    frame.iter().map(|s| f32::from_sample(*s)).sum::<f32>() / channels as f32;
                if producer
                    .push(if mono.is_finite() {
                        mono.clamp(-1.0, 1.0)
                    } else {
                        0.0
                    })
                    .is_err()
                {
                    dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
        },
        move |_| {
            failed.store(true, Ordering::Relaxed);
        },
        None,
    )?)
}

impl Microphone {
    pub fn open(name: Option<&str>) -> Result<Self> {
        let device = device(true, name)?;
        let supported = device
            .default_input_config()
            .context("cannot read the microphone's default audio configuration; check the selected input device")?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let rate = config.sample_rate.0;
        if !(8_000..=192_000).contains(&rate) || config.channels == 0 {
            bail!(
                "unsupported microphone configuration: {rate} Hz, {} channels",
                config.channels
            );
        }
        let (producer, consumer) = RingBuffer::new(rate as usize / 2); // At most 0.5 s raw audio.
        let enabled = Arc::new(AtomicBool::new(true));
        let failed = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicUsize::new(0));
        macro_rules! stream {
            ($ty:ty) => {
                input_stream::<$ty>(
                    &device,
                    &config,
                    producer,
                    enabled.clone(),
                    failed.clone(),
                    dropped.clone(),
                )
                .context(
                    "cannot build microphone stream; check the device and microphone permissions",
                )?
            };
        }
        let stream = match format {
            SampleFormat::F32 => stream!(f32),
            SampleFormat::F64 => stream!(f64),
            SampleFormat::I16 => stream!(i16),
            SampleFormat::U16 => stream!(u16),
            SampleFormat::I32 => stream!(i32),
            _ => bail!("unsupported microphone sample format: {format:?}"),
        };
        stream.play().context(
            "cannot start microphone stream; check the device and microphone permissions",
        )?;
        Ok(Self {
            _stream: stream,
            consumer,
            enabled,
            failed,
            dropped,
            sample_rate: rate,
        })
    }

    pub fn pop(&mut self) -> Option<f32> {
        self.consumer.pop().ok()
    }
    pub fn discard_pending(&mut self) {
        while self.consumer.pop().is_ok() {}
    }
    pub fn gate(&mut self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
        if !enabled {
            self.discard_pending();
        }
    }
    pub fn take_dropped(&self) -> usize {
        self.dropped.swap(0, Ordering::Relaxed)
    }
    pub fn check(&self) -> Result<()> {
        if self.failed.load(Ordering::Relaxed) {
            bail!("microphone stream failed; check the device and restart the listener");
        }
        Ok(())
    }
}

fn output_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut consumer: Consumer<f32>,
    remaining: Arc<AtomicUsize>,
    failed: Arc<AtomicBool>,
) -> Result<cpal::Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels);
    Ok(device.build_output_stream(
        config,
        move |data: &mut [T], _| {
            for frame in data.chunks_exact_mut(channels) {
                let value = match consumer.pop() {
                    Ok(value) => {
                        remaining.fetch_sub(1, Ordering::Release);
                        value
                    }
                    Err(_) => 0.0,
                };
                frame.fill(T::from_sample(value));
            }
        },
        move |_| {
            failed.store(true, Ordering::Relaxed);
        },
        None,
    )?)
}

impl Speaker {
    pub fn open(name: Option<&str>) -> Result<Self> {
        let device = device(false, name)?;
        let supported = device.default_output_config()?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let rate = config.sample_rate.0;
        if !(8_000..=192_000).contains(&rate) || config.channels == 0 {
            bail!(
                "unsupported output configuration: {rate} Hz, {} channels",
                config.channels
            );
        }
        let (producer, consumer) = RingBuffer::new(rate as usize * 5);
        let remaining = Arc::new(AtomicUsize::new(0));
        let failed = Arc::new(AtomicBool::new(false));
        macro_rules! stream {
            ($ty:ty) => {
                output_stream::<$ty>(
                    &device,
                    &config,
                    consumer,
                    remaining.clone(),
                    failed.clone(),
                )?
            };
        }
        let stream = match format {
            SampleFormat::F32 => stream!(f32),
            SampleFormat::F64 => stream!(f64),
            SampleFormat::I16 => stream!(i16),
            SampleFormat::U16 => stream!(u16),
            SampleFormat::I32 => stream!(i32),
            _ => bail!("unsupported output sample format: {format:?}"),
        };
        stream.play().context("cannot start audio output")?;
        Ok(Self {
            _stream: stream,
            producer,
            remaining,
            failed,
            sample_rate: rate,
        })
    }

    pub fn play(&mut self, samples: &[f32], running: &AtomicBool) -> Result<()> {
        if samples.len() > self.producer.slots() {
            bail!("generated clip exceeds audio output capacity");
        }
        self.remaining.store(samples.len(), Ordering::Release);
        for sample in samples {
            self.producer
                .push(*sample)
                .map_err(|_| anyhow::anyhow!("audio output queue full"))?;
        }
        while self.remaining.load(Ordering::Acquire) != 0 && running.load(Ordering::Relaxed) {
            if self.failed.load(Ordering::Relaxed) {
                bail!("audio output stream failed; check the device");
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        Ok(())
    }
}

/// Streaming low-pass FIR followed by fractional interpolation to 16 kHz.
/// State is fixed-size and independent of recording duration.
pub struct Resampler {
    taps: [f32; 63],
    history: [f32; 63],
    cursor: usize,
    index: u64,
    next: f64,
    step: f64,
    previous: f32,
}

impl Resampler {
    pub fn new(input_rate: u32) -> Self {
        let cutoff = (16_000.0 / f64::from(input_rate)).min(1.0) * 0.45;
        let mut taps = std::array::from_fn(|i| {
            let x = i as f64 - 31.0;
            let sinc = if x == 0.0 {
                2.0 * cutoff
            } else {
                (std::f64::consts::TAU * cutoff * x).sin() / (std::f64::consts::PI * x)
            };
            (sinc * (0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / 62.0).cos())) as f32
        });
        let sum: f32 = taps.iter().sum();
        for tap in &mut taps {
            *tap /= sum;
        }
        Self {
            taps,
            history: [0.0; 63],
            cursor: 0,
            index: 0,
            next: 0.0,
            step: f64::from(input_rate) / 16_000.0,
            previous: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.history.fill(0.0);
        self.cursor = 0;
        self.index = 0;
        self.next = 0.0;
        self.previous = 0.0;
    }

    pub fn push(&mut self, sample: f32) -> ([f32; 2], usize) {
        self.history[self.cursor] = sample;
        let mut filtered = 0.0;
        for (i, tap) in self.taps.iter().enumerate() {
            filtered += tap * self.history[(self.cursor + 63 - i) % 63];
        }
        self.cursor = (self.cursor + 1) % 63;
        let mut output = [0.0; 2];
        let mut count = 0;
        while self.next <= self.index as f64 && count < output.len() {
            let fraction = (self.next - self.index as f64 + 1.0).clamp(0.0, 1.0) as f32;
            output[count] = self.previous + (filtered - self.previous) * fraction;
            count += 1;
            self.next += self.step;
        }
        self.index += 1;
        self.previous = filtered;
        (output, count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn convert(rate: u32, frequency: f32) -> Vec<f32> {
        let mut r = Resampler::new(rate);
        let mut result = Vec::new();
        for i in 0..rate {
            let sample = (std::f32::consts::TAU * frequency * i as f32 / rate as f32).sin();
            let (output, count) = r.push(sample);
            result.extend_from_slice(&output[..count]);
        }
        result
    }

    #[test]
    fn resamples_common_device_rates_with_bounded_state() {
        for rate in [8_000, 16_000, 44_100, 48_000, 96_000, 192_000] {
            let samples = convert(rate, 180.0);
            assert!(
                (15_998..=16_001).contains(&samples.len()),
                "{rate}: {}",
                samples.len()
            );
            assert!(samples.iter().all(|s| s.is_finite() && s.abs() <= 1.1));
        }
    }

    #[test]
    fn downsampling_rejects_out_of_band_sound() {
        let rms = |a: Vec<f32>| {
            (a[200..].iter().map(|s| s * s).sum::<f32>() / (a.len() - 200) as f32).sqrt()
        };
        assert!(rms(convert(48_000, 12_000.0)) < rms(convert(48_000, 1000.0)) * 0.05);
    }

    #[test]
    fn reset_discards_previous_audio() {
        let mut r = Resampler::new(48_000);
        for _ in 0..100 {
            r.push(1.0);
        }
        r.reset();
        for _ in 0..100 {
            let (out, n) = r.push(0.0);
            assert!(out[..n].iter().all(|v| *v == 0.0));
        }
    }
}
