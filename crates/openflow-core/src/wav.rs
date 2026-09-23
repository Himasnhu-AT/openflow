//! WAV helpers. Clips are 32-bit float 16 kHz mono (AVAudioFile in the Swift app); uploads to Groq and
//! whisper-server are 16-bit PCM with a 44-byte header (Cloud.wav).

use std::path::Path;

use crate::SAMPLE_RATE;

pub fn write_f32(path: &Path, samples: &[f32]) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE as u32,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(path, spec)?;
    for s in samples {
        w.write_sample(*s)?;
    }
    w.finalize()?;
    Ok(())
}

/// Reads a mono clip as f32 (float or integer PCM). Assumes 16 kHz, like every clip OpenFlow writes.
pub fn read(path: &Path) -> anyhow::Result<Vec<f32>> {
    let mut r = hound::WavReader::open(path)?;
    let spec = r.spec();
    let ch = spec.channels.max(1) as usize;
    let all: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = (1u64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>().map(|s| s.map(|v| v as f32 / scale)).collect::<Result<_, _>>()?
        }
    };
    Ok(all.chunks(ch).map(|c| c[0]).collect())
}

/// 16-bit PCM WAV bytes (samples clamped to [-1, 1] * 32767), 44-byte header.
pub fn pcm16_bytes(samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&(SAMPLE_RATE as u32).to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE as u32 * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm16_header() {
        let b = pcm16_bytes(&[0.0, 1.0, -2.0]);
        assert_eq!(b.len(), 44 + 6);
        assert_eq!(&b[0..4], b"RIFF");
        assert_eq!(i16::from_le_bytes([b[46], b[47]]), 32767);
        assert_eq!(i16::from_le_bytes([b[48], b[49]]), -32767);
    }
}
