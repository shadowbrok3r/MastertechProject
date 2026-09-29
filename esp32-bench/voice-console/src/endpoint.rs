//! End-of-speech detection for hands-free utterances against a measured room-noise baseline.

use std::collections::VecDeque;

/// Level frame length: 512 samples at 16 kHz.
pub const FRAME_MS: u32 = 32;
/// Newest frames kept out of the baseline, so a just-spoken wake word never counts as noise.
const RECENT_FRAMES: usize = 47;
/// Baseline window: 6 s of quiet frames older than the recent window.
const BASELINE_FRAMES: usize = 188;
const MIN_BASELINE_FRAMES: usize = 31;
/// Speech must clear the room's median level by this much.
const MIN_MARGIN_DB: f32 = 4.0;
/// Speech must clear the room's loud moments (90th percentile) by this much.
const PEAK_MARGIN_DB: f32 = 3.0;
/// The gate stays this far below the talker's own level.
const TALKER_HEADROOM_DB: f32 = 6.0;
/// Lowest gate, for near-silent rooms.
const QUIET_GATE_DB: f32 = -60.0;
/// Frames this far over the gate count as speech even when the VAD disagrees.
const STRONG_DB: f32 = 10.0;
/// Frames right after the wake word that are ignored.
const GRACE_MS: u32 = 256;
const MIN_SPEECH_MS: u32 = 160;
const END_SILENCE_MS: u32 = 800;
const NO_SPEECH_MS: u32 = 5_000;
const MAX_MS: u32 = 15_000;

#[derive(Clone, Copy, Debug)]
pub struct Frame {
    /// MIC1+MIC2 level in dBFS.
    pub db: f32,
    /// The VAD reported speech.
    pub speech: bool,
}

/// Room-noise statistics over the baseline window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Noise {
    pub median_db: f32,
    pub p90_db: f32,
}

impl Noise {
    /// Level a frame must reach to count as the talker, capped below `talker_db` when known.
    pub fn gate(&self, talker_db: Option<f32>) -> f32 {
        let floor = self.median_db + MIN_MARGIN_DB;
        let mut gate = (self.p90_db + PEAK_MARGIN_DB).max(floor);
        if let Some(talker) = talker_db {
            gate = gate.min(talker - TALKER_HEADROOM_DB).max(floor);
        }
        gate.max(QUIET_GATE_DB)
    }
}

/// Rolling measurement of the room's noise level.
#[derive(Default)]
pub struct NoiseBaseline {
    recent: VecDeque<(f32, bool)>,
    quiet: VecDeque<f32>,
}

impl NoiseBaseline {
    /// Adds a frame; `quiet` frames (nothing playing, mic closed) join the baseline once they leave the recent window.
    pub fn push(&mut self, db: f32, quiet: bool) {
        self.recent.push_back((db, quiet));
        if self.recent.len() <= RECENT_FRAMES {
            return;
        }
        if let Some((old, true)) = self.recent.pop_front() {
            self.quiet.push_back(old);
            if self.quiet.len() > BASELINE_FRAMES {
                self.quiet.pop_front();
            }
        }
    }

    /// The room's noise, once 1 s of quiet frames has been measured.
    pub fn noise(&self) -> Option<Noise> {
        if self.quiet.len() < MIN_BASELINE_FRAMES {
            return None;
        }
        let sorted = sorted(self.quiet.iter().copied());
        Some(Noise { median_db: percentile(&sorted, 0.5), p90_db: percentile(&sorted, 0.9) })
    }

    /// 80th-percentile level of the recent window: the wake word just spoken.
    pub fn recent_db(&self) -> Option<f32> {
        (self.recent.len() >= RECENT_FRAMES / 2).then(|| percentile(&sorted(self.recent.iter().map(|r| r.0)), 0.8))
    }
}

fn sorted(levels: impl Iterator<Item = f32>) -> Vec<f32> {
    let mut v: Vec<f32> = levels.collect();
    v.sort_by(f32::total_cmp);
    v
}

fn percentile(sorted: &[f32], p: f32) -> f32 {
    sorted[((sorted.len() - 1) as f32 * p).round() as usize]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    /// Speech, then `END_SILENCE_MS` without it.
    Spoke,
    NoSpeech,
    TooLong,
}

impl End {
    pub fn as_str(self) -> &'static str {
        match self {
            End::Spoke => "spoke",
            End::NoSpeech => "no_speech",
            End::TooLong => "too_long",
        }
    }
}

/// Follows one hands-free utterance frame by frame; without a gate the VAD alone decides.
pub struct Endpointer {
    gate: Option<f32>,
    elapsed_ms: u32,
    speech_ms: u32,
    silence_ms: u32,
    speech_db_sum: f32,
}

impl Endpointer {
    pub fn new(gate: Option<f32>) -> Self {
        Self { gate, elapsed_ms: 0, speech_ms: 0, silence_ms: 0, speech_db_sum: 0.0 }
    }

    pub fn gate(&self) -> Option<f32> {
        self.gate
    }

    pub fn elapsed_ms(&self) -> u32 {
        self.elapsed_ms
    }

    pub fn speech_ms(&self) -> u32 {
        self.speech_ms
    }

    pub fn heard(&self) -> bool {
        self.speech_ms >= MIN_SPEECH_MS
    }

    /// Mean level of the talker's frames, once heard.
    pub fn talker_db(&self) -> Option<f32> {
        self.heard().then(|| self.speech_db_sum / (self.speech_ms / FRAME_MS) as f32)
    }

    pub fn feed(&mut self, f: Frame) -> Option<End> {
        self.elapsed_ms += FRAME_MS;
        if self.elapsed_ms <= GRACE_MS {
            return None;
        }
        if self.voiced(f) {
            self.speech_ms += FRAME_MS;
            self.speech_db_sum += f.db;
            self.silence_ms = 0;
        } else {
            self.silence_ms += FRAME_MS;
        }
        if self.heard() && self.silence_ms >= END_SILENCE_MS {
            Some(End::Spoke)
        } else if !self.heard() && self.elapsed_ms >= NO_SPEECH_MS {
            Some(End::NoSpeech)
        } else if self.elapsed_ms >= MAX_MS {
            Some(End::TooLong)
        } else {
            None
        }
    }

    fn voiced(&self, f: Frame) -> bool {
        match self.gate {
            Some(gate) => f.db >= gate && (f.speech || f.db >= gate + STRONG_DB),
            None => f.speech,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WAKE_TAIL: usize = (GRACE_MS / FRAME_MS) as usize;

    /// Deterministic ±`spread` dB jitter around `db`.
    fn jitter(db: f32, spread: f32, i: usize) -> f32 {
        let phase = (i * 7919 % 101) as f32 / 100.0;
        db + spread * (2.0 * phase - 1.0)
    }

    fn baseline_of(db: f32, spread: f32, secs: usize) -> NoiseBaseline {
        let mut b = NoiseBaseline::default();
        for i in 0..secs * 1000 / FRAME_MS as usize {
            b.push(jitter(db, spread, i), true);
        }
        b
    }

    /// Feeds frames until an end; returns it with the elapsed ms.
    fn run(ep: &mut Endpointer, frames: impl IntoIterator<Item = Frame>) -> Option<(End, u32)> {
        frames.into_iter().find_map(|f| ep.feed(f)).map(|end| (end, ep.elapsed_ms()))
    }

    fn frames(n_ms: u32, db: f32, spread: f32, speech: bool) -> Vec<Frame> {
        (0..(n_ms / FRAME_MS) as usize).map(|i| Frame { db: jitter(db, spread, i), speech }).collect()
    }

    fn span_ms(frames: &[Frame]) -> u32 {
        frames.len() as u32 * FRAME_MS
    }

    #[test]
    fn baseline_needs_a_second_of_quiet() {
        let mut b = NoiseBaseline::default();
        for _ in 0..RECENT_FRAMES + MIN_BASELINE_FRAMES - 1 {
            b.push(-55.0, true);
        }
        assert_eq!(b.noise(), None);
        b.push(-55.0, true);
        assert!(b.noise().is_some());
    }

    #[test]
    fn baseline_skips_playback_and_the_wake_word() {
        let mut b = baseline_of(-55.0, 1.0, 3);
        for _ in 0..30 {
            b.push(-25.0, false);
        }
        for _ in 0..RECENT_FRAMES {
            b.push(-30.0, true);
        }
        let noise = b.noise().unwrap();
        assert!((noise.median_db + 55.0).abs() < 1.5, "median {}", noise.median_db);
        assert!(noise.p90_db < -53.0, "p90 {}", noise.p90_db);
        assert!((b.recent_db().unwrap() + 30.0).abs() < 0.1);
    }

    #[test]
    fn baseline_follows_a_louder_room() {
        let mut b = baseline_of(-55.0, 1.0, 6);
        for i in 0..8 * 1000 / FRAME_MS as usize {
            b.push(jitter(-40.0, 1.0, i), true);
        }
        assert!((b.noise().unwrap().median_db + 40.0).abs() < 1.5);
    }

    #[test]
    fn quiet_room_gate_sits_above_the_noise() {
        let noise = Noise { median_db: -55.0, p90_db: -53.0 };
        assert_eq!(noise.gate(None), -50.0);
        assert_eq!(noise.gate(Some(-30.0)), -50.0);
        let silent = Noise { median_db: -75.0, p90_db: -73.0 };
        assert_eq!(silent.gate(None), QUIET_GATE_DB);
    }

    #[test]
    fn loud_room_gate_stays_below_the_talker() {
        let noise = Noise { median_db: -40.0, p90_db: -30.0 };
        assert_eq!(noise.gate(None), -27.0);
        assert_eq!(noise.gate(Some(-28.0)), -34.0);
        // Never below the room's median plus margin, however quiet the talker.
        assert_eq!(noise.gate(Some(-45.0)), -36.0);
    }

    #[test]
    fn ends_soon_after_the_talker_stops_despite_vad_hangover() {
        let noise = baseline_of(-55.0, 2.0, 6).noise().unwrap();
        let mut ep = Endpointer::new(Some(noise.gate(None)));
        let mut input = frames(1_500, -32.0, 4.0, true);
        let spoken = span_ms(&input);
        input.extend(frames(700, -55.0, 2.0, true));
        input.extend(frames(3_000, -55.0, 2.0, false));
        assert_eq!(run(&mut ep, input), Some((End::Spoke, spoken + END_SILENCE_MS)));
    }

    #[test]
    fn store_chatter_does_not_hold_the_utterance_open() {
        let noise = baseline_of(-42.0, 4.0, 6).noise().unwrap();
        let mut ep = Endpointer::new(Some(noise.gate(Some(-28.0))));
        let mut input = frames(2_000, -28.0, 3.0, true);
        let spoken = span_ms(&input);
        input.extend(frames(10_000, -42.0, 4.0, true));
        assert_eq!(run(&mut ep, input), Some((End::Spoke, spoken + END_SILENCE_MS)));
        assert!((ep.talker_db().unwrap() + 28.0).abs() < 1.5);
    }

    #[test]
    fn without_a_baseline_the_vad_decides() {
        let mut ep = Endpointer::new(None);
        let mut input = frames(1_000, -55.0, 0.0, true);
        let spoken = span_ms(&input);
        input.extend(frames(3_000, -30.0, 0.0, false));
        assert_eq!(run(&mut ep, input), Some((End::Spoke, spoken + END_SILENCE_MS)));
    }

    #[test]
    fn a_wake_with_nothing_after_it_is_no_speech() {
        let noise = baseline_of(-50.0, 3.0, 6).noise().unwrap();
        let mut ep = Endpointer::new(Some(noise.gate(None)));
        let (end, ms) = run(&mut ep, frames(10_000, -50.0, 3.0, true)).unwrap();
        assert_eq!(end, End::NoSpeech);
        assert!((NO_SPEECH_MS..NO_SPEECH_MS + FRAME_MS).contains(&ms));
        assert_eq!(ep.talker_db(), None);
    }

    #[test]
    fn the_wake_word_tail_is_ignored() {
        let mut ep = Endpointer::new(Some(-50.0));
        let mut input = vec![Frame { db: -25.0, speech: true }; WAKE_TAIL];
        input.extend(frames(6_000, -60.0, 0.0, false));
        assert_eq!(run(&mut ep, input).unwrap().0, End::NoSpeech);
    }

    #[test]
    fn strong_frames_count_without_the_vad() {
        let mut ep = Endpointer::new(Some(-50.0));
        let mut input = frames(1_000, -36.0, 0.0, false);
        input.extend(frames(2_000, -60.0, 0.0, false));
        assert_eq!(run(&mut ep, input).unwrap().0, End::Spoke);
        let mut weak = Endpointer::new(Some(-50.0));
        assert_eq!(run(&mut weak, frames(6_000, -45.0, 0.0, false)).unwrap().0, End::NoSpeech);
    }

    #[test]
    fn nonstop_speech_is_cut_at_the_limit() {
        let mut ep = Endpointer::new(Some(-50.0));
        let (end, ms) = run(&mut ep, frames(20_000, -30.0, 0.0, true)).unwrap();
        assert_eq!(end, End::TooLong);
        assert!((MAX_MS..MAX_MS + FRAME_MS).contains(&ms));
    }
}
