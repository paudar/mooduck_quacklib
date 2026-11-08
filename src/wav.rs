use crate::Audio;
use std::io::{self, Write};

pub(crate) fn write(audio: &Audio, mut writer: impl Write) -> io::Result<()> {
    let data_len = u32::try_from(audio.samples().len())
        .ok()
        .and_then(|n| n.checked_mul(2))
        .filter(|n| *n <= u32::MAX - 36)
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "clip exceeds WAV size limit")
        })?;
    writer.write_all(b"RIFF")?;
    writer.write_all(&(36 + data_len).to_le_bytes())?;
    writer.write_all(b"WAVEfmt ")?;
    writer.write_all(&16_u32.to_le_bytes())?;
    writer.write_all(&1_u16.to_le_bytes())?; // PCM
    writer.write_all(&1_u16.to_le_bytes())?; // mono
    writer.write_all(&audio.sample_rate().to_le_bytes())?;
    writer.write_all(&(audio.sample_rate() * 2).to_le_bytes())?;
    writer.write_all(&2_u16.to_le_bytes())?;
    writer.write_all(&16_u16.to_le_bytes())?;
    writer.write_all(b"data")?;
    writer.write_all(&data_len.to_le_bytes())?;
    for sample in audio.samples() {
        writer.write_all(&((sample * 32767.0).round() as i16).to_le_bytes())?;
    }
    Ok(())
}
