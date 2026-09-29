//! Piper voices: the installed catalog, synthesis settings, synthesis, and the board's persisted voice.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::RwLock;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::pipeline::env_or;

const DEFAULT_VOICE: &str = "en_US-lessac-medium";

fn voices_dir() -> PathBuf {
    PathBuf::from(env_or("PIPER_VOICES", "/home/shadowbroker/voice/piper/voices"))
}

fn settings_path() -> PathBuf {
    PathBuf::from(env_or("VB_VOICE_FILE", "/home/shadowbroker/.config/voice-bridge/voice.json"))
}

fn default_length_scale() -> f32 {
    1.0
}

fn default_noise_scale() -> f32 {
    0.667
}

fn default_noise_w() -> f32 {
    0.8
}

/// A voice (model file stem) and Piper's pace and variation knobs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VoiceSettings {
    pub voice: String,
    #[serde(default)]
    pub speaker: u32,
    #[serde(default = "default_length_scale")]
    pub length_scale: f32,
    #[serde(default = "default_noise_scale")]
    pub noise_scale: f32,
    #[serde(default = "default_noise_w")]
    pub noise_w: f32,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self {
            voice: DEFAULT_VOICE.to_string(),
            speaker: 0,
            length_scale: default_length_scale(),
            noise_scale: default_noise_scale(),
            noise_w: default_noise_w(),
        }
    }
}

impl VoiceSettings {
    /// Checks the voice is installed and clamps the speaker and knobs to usable ranges.
    pub fn validated(mut self, catalog: &[VoiceInfo]) -> Result<Self> {
        let info = catalog
            .iter()
            .find(|v| v.id == self.voice)
            .with_context(|| format!("voice {} is not installed", self.voice))?;
        self.speaker = self.speaker.min(info.speakers.len().saturating_sub(1) as u32);
        self.length_scale = self.length_scale.clamp(0.5, 2.0);
        self.noise_scale = self.noise_scale.clamp(0.0, 1.5);
        self.noise_w = self.noise_w.clamp(0.0, 1.5);
        Ok(self)
    }
}

/// An installed voice and the defaults from its model config.
#[derive(Clone, Debug, Serialize)]
pub struct VoiceInfo {
    pub id: String,
    pub dataset: String,
    pub language: String,
    pub region: String,
    pub quality: String,
    pub sample_rate: u32,
    /// Speaker names by id; empty for a single-speaker voice.
    pub speakers: Vec<String>,
    pub length_scale: f32,
    pub noise_scale: f32,
    pub noise_w: f32,
}

impl VoiceInfo {
    fn from_config(id: String, cfg: &Value) -> Self {
        let count = cfg["num_speakers"].as_u64().unwrap_or(1) as usize;
        let mut speakers = vec![String::new(); if count > 1 { count } else { 0 }];
        if let Some(map) = cfg["speaker_id_map"].as_object() {
            for (name, sid) in map {
                if let Some(slot) = sid.as_u64().and_then(|i| speakers.get_mut(i as usize)) {
                    *slot = name.clone();
                }
            }
        }
        let text = |v: &Value| v.as_str().unwrap_or_default().to_string();
        let knob = |key: &str, default: f32| cfg["inference"][key].as_f64().map_or(default, |v| v as f32);
        Self {
            dataset: cfg["dataset"].as_str().map_or_else(|| id.clone(), str::to_string),
            language: text(&cfg["language"]["code"]),
            region: text(&cfg["language"]["country_english"]),
            quality: text(&cfg["audio"]["quality"]),
            sample_rate: cfg["audio"]["sample_rate"].as_u64().unwrap_or(22_050) as u32,
            speakers,
            length_scale: knob("length_scale", default_length_scale()),
            noise_scale: knob("noise_scale", default_noise_scale()),
            noise_w: knob("noise_w", default_noise_w()),
            id,
        }
    }
}

/// Voices with both an `.onnx` model and its `.onnx.json` config, sorted by id.
pub fn catalog() -> Vec<VoiceInfo> {
    let Ok(dir) = std::fs::read_dir(voices_dir()) else {
        return Vec::new();
    };
    let mut voices: Vec<VoiceInfo> = dir
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let id = entry.file_name().to_str()?.strip_suffix(".onnx")?.to_string();
            let cfg = std::fs::read_to_string(entry.path().with_extension("onnx.json")).ok()?;
            Some(VoiceInfo::from_config(id, &serde_json::from_str(&cfg).ok()?))
        })
        .collect();
    voices.sort_by(|a, b| a.id.cmp(&b.id));
    voices
}

/// Piper's WAV output for `text` in `v`, with markdown marks dropped and lines joined.
pub fn synthesize(text: &str, v: &VoiceSettings) -> Result<Vec<u8>> {
    let bin = env_or("PIPER_BIN", "/home/shadowbroker/voice/piper/piper/piper");
    let mut cmd = Command::new(&bin);
    cmd.arg("--model")
        .arg(voices_dir().join(format!("{}.onnx", v.voice)))
        .args(["--output_file", "-", "--quiet"])
        .args(["--length_scale", &v.length_scale.to_string()])
        .args(["--noise_scale", &v.noise_scale.to_string()])
        .args(["--noise_w", &v.noise_w.to_string()]);
    if v.speaker > 0 {
        cmd.args(["--speaker", &v.speaker.to_string()]);
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("running {bin}"))?;
    let spoken: String = text
        .chars()
        .filter(|c| !matches!(c, '*' | '`' | '#'))
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    child.stdin.take().context("piper stdin")?.write_all(spoken.as_bytes())?;
    let out = child.wait_with_output()?;
    if !out.status.success() || out.stdout.is_empty() {
        bail!("piper failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(out.stdout)
}

/// The voice the board's replies use, saved to `VB_VOICE_FILE`.
pub struct ActiveVoice(RwLock<VoiceSettings>);

impl ActiveVoice {
    pub fn load() -> Self {
        let saved = std::fs::read_to_string(settings_path())
            .ok()
            .and_then(|s| serde_json::from_str::<VoiceSettings>(&s).ok())
            .and_then(|v| v.validated(&catalog()).ok());
        Self(RwLock::new(saved.unwrap_or_default()))
    }

    pub fn get(&self) -> VoiceSettings {
        self.0.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn set(&self, v: VoiceSettings) -> Result<()> {
        let path = settings_path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, serde_json::to_string_pretty(&v)?)?;
        *self.0.write().unwrap_or_else(|e| e.into_inner()) = v;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn info(id: &str, speakers: usize) -> VoiceInfo {
        let cfg = json!({ "num_speakers": speakers, "audio": { "quality": "medium" } });
        VoiceInfo::from_config(id.to_string(), &cfg)
    }

    #[test]
    fn config_maps_speaker_names_by_id() {
        let cfg = json!({
            "dataset": "semaine",
            "num_speakers": 3,
            "speaker_id_map": { "prudence": 0, "spike": 1, "obadiah": 2 },
            "language": { "code": "en_GB", "country_english": "Great Britain" },
            "audio": { "sample_rate": 22050, "quality": "medium" },
            "inference": { "noise_scale": 0.5 }
        });
        let v = VoiceInfo::from_config("en_GB-semaine-medium".into(), &cfg);
        assert_eq!(v.speakers, ["prudence", "spike", "obadiah"]);
        assert_eq!(v.region, "Great Britain");
        assert_eq!(v.noise_scale, 0.5);
        assert_eq!(v.noise_w, default_noise_w());
    }

    #[test]
    fn single_speaker_voices_list_no_speakers() {
        assert!(info("en_US-amy-medium", 1).speakers.is_empty());
    }

    #[test]
    fn validation_rejects_unknown_voices_and_clamps_knobs() {
        let catalog = [info("en_US-amy-medium", 1), info("en_GB-vctk-medium", 109)];
        let unknown = VoiceSettings { voice: "../../etc/passwd".into(), ..Default::default() };
        assert!(unknown.validated(&catalog).is_err());

        let wild = VoiceSettings {
            voice: "en_US-amy-medium".into(),
            speaker: 7,
            length_scale: 9.0,
            noise_scale: -1.0,
            noise_w: 3.0,
        };
        let v = wild.validated(&catalog).unwrap();
        assert_eq!((v.speaker, v.length_scale, v.noise_scale, v.noise_w), (0, 2.0, 0.0, 1.5));

        let multi = VoiceSettings { voice: "en_GB-vctk-medium".into(), speaker: 500, ..Default::default() };
        assert_eq!(multi.validated(&catalog).unwrap().speaker, 108);
    }
}
