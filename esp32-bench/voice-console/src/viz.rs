//! Per-mic visuals: a peak-preserving waveform and log-spaced FFT spectrum bars.

use microfft::real::rfft_512;

/// Samples per mic per frame (32 ms at 16 kHz).
pub const WINDOW: usize = 512;
pub const WAVE_POINTS: usize = 64;
pub const BANDS: usize = 16;

const SAMPLE_RATE: f32 = 16_000.0;
/// Sample magnitude drawn at full waveform height (-12 dBFS).
const WAVE_FULL_SCALE: f32 = 8_192.0;
const BAND_LO_HZ: f32 = 90.0;
const BAND_HI_HZ: f32 = 7_000.0;
/// Bar range in dB relative to a full-scale sine.
const DB_FLOOR: f32 = -70.0;
const DB_CEIL: f32 = -10.0;
/// Bar drop per frame, out of 100.
const BAR_FALL: u8 = 6;

/// Waveform points in -100..=100; each is the largest-magnitude sample of its span.
pub fn waveform(samples: &[i16; WINDOW]) -> [i16; WAVE_POINTS] {
    let mut out = [0i16; WAVE_POINTS];
    for (point, span) in out.iter_mut().zip(samples.chunks_exact(WINDOW / WAVE_POINTS)) {
        let peak = span.iter().copied().max_by_key(|s| s.unsigned_abs()).unwrap_or(0);
        *point = (f32::from(peak) * 100.0 / WAVE_FULL_SCALE).clamp(-100.0, 100.0) as i16;
    }
    out
}

/// Spectrum bar heights (0..=100) for `BANDS` log-spaced bands.
pub fn bands(samples: &[i16; WINDOW]) -> [u8; BANDS] {
    let mut buf = [0f32; WINDOW];
    for (i, (b, s)) in buf.iter_mut().zip(samples).enumerate() {
        let hann = 0.5 - 0.5 * (core::f32::consts::TAU * i as f32 / WINDOW as f32).cos();
        *b = f32::from(*s) * hann;
    }
    let spectrum = rfft_512(&mut buf);
    // One-sided bin magnitude of a full-scale sine under a Hann window.
    let reference = 32_767.0 * WINDOW as f32 / 4.0;
    let mut out = [0u8; BANDS];
    for (k, bar) in out.iter_mut().enumerate() {
        let (lo, hi) = band_bins(k);
        let peak = spectrum[lo..hi].iter().map(|c| c.norm_sqr()).fold(0.0f32, f32::max).sqrt();
        let db = 20.0 * (peak / reference).max(1e-9).log10();
        *bar = ((db - DB_FLOOR) / (DB_CEIL - DB_FLOOR) * 100.0).clamp(0.0, 100.0) as u8;
    }
    out
}

/// FFT bin range `[lo, hi)` for band `k`, at least one bin wide.
fn band_bins(k: usize) -> (usize, usize) {
    let bin_hz = SAMPLE_RATE / WINDOW as f32;
    let edge = |i: usize| BAND_LO_HZ * (BAND_HI_HZ / BAND_LO_HZ).powf(i as f32 / BANDS as f32);
    let lo = ((edge(k) / bin_hz).floor() as usize).max(1);
    let hi = ((edge(k + 1) / bin_hz).ceil() as usize).clamp(lo + 1, WINDOW / 2);
    (lo, hi)
}

/// Bar heights that rise at once and fall by `BAR_FALL` per frame.
#[derive(Default)]
pub struct Bars([u8; BANDS]);

impl Bars {
    pub fn update(&mut self, target: &[u8; BANDS]) -> [u8; BANDS] {
        for (level, &t) in self.0.iter_mut().zip(target) {
            *level = t.max(level.saturating_sub(BAR_FALL));
        }
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(hz: f32, amplitude: f32) -> [i16; WINDOW] {
        let mut s = [0i16; WINDOW];
        for (i, v) in s.iter_mut().enumerate() {
            *v = (amplitude * (core::f32::consts::TAU * hz * i as f32 / SAMPLE_RATE).sin()) as i16;
        }
        s
    }

    fn loudest(b: &[u8; BANDS]) -> usize {
        (0..BANDS).max_by_key(|&k| b[k]).unwrap()
    }

    #[test]
    fn silence_draws_flat() {
        assert_eq!(waveform(&[0; WINDOW]), [0; WAVE_POINTS]);
        assert_eq!(bands(&[0; WINDOW]), [0; BANDS]);
    }

    #[test]
    fn waveform_keeps_each_spans_signed_peak() {
        let mut s = [0i16; WINDOW];
        s[3] = -8_192;
        s[9] = 4_096;
        s[10] = -100;
        let w = waveform(&s);
        assert_eq!(w[0], -100);
        assert_eq!(w[1], 50);
        assert_eq!(w[2], 0);
    }

    #[test]
    fn waveform_clamps_past_full_scale() {
        assert_eq!(waveform(&[i16::MAX; WINDOW])[0], 100);
        assert_eq!(waveform(&[i16::MIN; WINDOW])[0], -100);
    }

    #[test]
    fn a_tone_lights_its_own_band() {
        let b = bands(&tone(1_000.0, 16_000.0));
        let hot = loudest(&b);
        let (lo, hi) = band_bins(hot);
        let bin = (1_000.0 / (SAMPLE_RATE / WINDOW as f32)).round() as usize;
        assert!((lo..hi).contains(&bin), "1 kHz landed in band {hot} ({lo}..{hi})");
        assert!(b[hot] >= 90, "tone band only reached {}", b[hot]);
        assert!(b[0] < 20 && b[BANDS - 1] < 20, "far bands lit: {b:?}");
    }

    #[test]
    fn quieter_tones_draw_shorter_bars() {
        let loud = bands(&tone(2_000.0, 16_000.0));
        let quiet = bands(&tone(2_000.0, 500.0));
        let k = loudest(&loud);
        assert!(quiet[k] > 0 && quiet[k] < loud[k], "loud {} quiet {}", loud[k], quiet[k]);
    }

    #[test]
    fn bands_cover_the_range_without_gaps() {
        let mut prev_hi = 0;
        for k in 0..BANDS {
            let (lo, hi) = band_bins(k);
            assert!(lo < hi, "band {k} is empty");
            if k > 0 {
                assert!(lo <= prev_hi, "gap before band {k}: {prev_hi}..{lo}");
            }
            prev_hi = hi;
        }
        assert!(prev_hi <= WINDOW / 2);
    }

    #[test]
    fn bars_rise_at_once_and_fall_slowly() {
        let mut bars = Bars::default();
        let mut peak = [0u8; BANDS];
        peak[4] = 90;
        assert_eq!(bars.update(&peak)[4], 90);
        assert_eq!(bars.update(&[0; BANDS])[4], 90 - BAR_FALL);
    }
}
