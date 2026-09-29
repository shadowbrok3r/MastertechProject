//! PCM16 mono WAV read/write, sample-rate conversion, and byte packing for the
//! relay's binary audio frames.

use anyhow::{bail, Result};
use std::io::{Read, Write};

pub const BENCH_RATE: u32 = 16_000;

pub fn i16_to_le_bytes(samples: &[i16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

pub fn i16_from_le_bytes(bytes: &[u8]) -> Vec<i16> {
    bytes
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect()
}

pub fn write_wav_mono(path: &str, samples: &[i16], sample_rate: u32) -> Result<()> {
    let data_len = (samples.len() * 2) as u32;
    let byte_rate = sample_rate * 2;
    let mut f = std::fs::File::create(path)?;
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data_len).to_le_bytes())?;
    f.write_all(b"WAVE")?;
    f.write_all(b"fmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?; // PCM
    f.write_all(&1u16.to_le_bytes())?; // mono
    f.write_all(&sample_rate.to_le_bytes())?;
    f.write_all(&byte_rate.to_le_bytes())?;
    f.write_all(&2u16.to_le_bytes())?; // block align
    f.write_all(&16u16.to_le_bytes())?; // bits per sample
    f.write_all(b"data")?;
    f.write_all(&data_len.to_le_bytes())?;
    f.write_all(&i16_to_le_bytes(samples))?;
    Ok(())
}

/// Reads a PCM16 WAV, returning its sample rate and mono samples (channel 0 of any layout).
pub fn read_wav_mono(path: &str) -> Result<(u32, Vec<i16>)> {
    let mut buf = Vec::new();
    std::fs::File::open(path)?.read_to_end(&mut buf)?;
    if buf.len() < 12 || &buf[0..4] != b"RIFF" || &buf[8..12] != b"WAVE" {
        bail!("not a RIFF/WAVE file");
    }
    let mut pos = 12;
    let mut rate = 0u32;
    let mut channels = 1u16;
    let mut bits = 16u16;
    let mut data: Option<&[u8]> = None;
    while pos + 8 <= buf.len() {
        let id = &buf[pos..pos + 4];
        let size = u32::from_le_bytes([buf[pos + 4], buf[pos + 5], buf[pos + 6], buf[pos + 7]]) as usize;
        let body = pos + 8;
        let end = (body + size).min(buf.len());
        match id {
            b"fmt " if size >= 16 => {
                channels = u16::from_le_bytes([buf[body + 2], buf[body + 3]]);
                rate = u32::from_le_bytes([buf[body + 4], buf[body + 5], buf[body + 6], buf[body + 7]]);
                bits = u16::from_le_bytes([buf[body + 14], buf[body + 15]]);
            }
            b"data" => data = Some(&buf[body..end]),
            _ => {}
        }
        pos = body + size + (size & 1); // chunks are word-aligned
    }
    if bits != 16 {
        bail!("only 16-bit PCM is supported (got {bits}-bit)");
    }
    let data = data.ok_or_else(|| anyhow::anyhow!("no data chunk"))?;
    let interleaved = i16_from_le_bytes(data);
    let mono = if channels <= 1 {
        interleaved
    } else {
        interleaved.iter().step_by(channels as usize).copied().collect()
    };
    if rate == 0 {
        bail!("no fmt chunk / zero sample rate");
    }
    Ok((rate, mono))
}

/// Linear-interpolation resample. Adequate for 22050->16000 speech; a low-pass
/// could be added if aliasing shows up.
pub fn resample(input: &[i16], from: u32, to: u32) -> Vec<i16> {
    if from == to || input.is_empty() {
        return input.to_vec();
    }
    let ratio = to as f64 / from as f64;
    let out_len = ((input.len() as f64) * ratio).round() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src = i as f64 / ratio;
        let idx = src.floor() as usize;
        let frac = src - idx as f64;
        let a = input.get(idx).copied().unwrap_or(0) as f64;
        let b = input.get(idx + 1).copied().unwrap_or(a as i16) as f64;
        out.push((a + (b - a) * frac).round() as i16);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_roundtrips() {
        let samples: Vec<i16> = (0..1000).map(|i| ((i * 37) % 2000 - 1000) as i16).collect();
        let path = std::env::temp_dir().join("vb_audio_test.wav");
        let path = path.to_str().unwrap();
        write_wav_mono(path, &samples, BENCH_RATE).unwrap();
        let (rate, got) = read_wav_mono(path).unwrap();
        assert_eq!(rate, BENCH_RATE);
        assert_eq!(got, samples);
    }

    #[test]
    fn resample_scales_length() {
        let input = vec![0i16; 22050];
        let out = resample(&input, 22050, 16000);
        assert!((out.len() as i32 - 16000).abs() <= 1);
    }

    #[test]
    fn resample_is_identity_at_same_rate() {
        let input: Vec<i16> = (0..100).map(|i| i as i16).collect();
        assert_eq!(resample(&input, 16000, 16000), input);
    }
}
