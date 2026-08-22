//! Canonical `who did this` stamp for provenance fields.
//!
//! `diagnostic_session.tech` held twelve spellings of one idea — "Claude (AI)",
//! "Claude (Logan)", "Claude (logan.lees)", "Logan (Claude-assisted)", … —
//! because every caller free-texted its own identity. This is the only way to
//! build one, and `Display` is the only wire format.
//!
//! Shape: `<harness>/<actor>[@<model>][#<node>]`
//!
//! - `mcp/logan.lees@claude-opus-5#DESKTOP-EI5PV29`
//! - `zeroclaw/sweeper@qwen3.8-unsloth-q6_k_xl#node-10`
//! - `tech/tyler.naylor`
//!
//! `harness` and `node` are meant to be filled in by the MCP server from the
//! caller's `client_info` and the local hostname, not by the agent: an agent
//! asserting its own runtime is exactly what drifted.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Where the work ran. Closed set so the prefix stays groupable in SQL.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Harness {
    /// A client speaking to the MasterTech MCP server directly.
    Mcp,
    /// Spawned by the zeroclaw daemon (agent turn, webhook, or channel).
    Zeroclaw,
    /// Unattended schedule.
    Cron,
    /// A human, no model involved.
    Tech,
    /// Written before this convention existed and not safely inferable.
    Legacy,
}

impl Harness {
    pub const ALL: [Self; 5] = [Self::Mcp, Self::Zeroclaw, Self::Cron, Self::Tech, Self::Legacy];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mcp => "mcp",
            Self::Zeroclaw => "zeroclaw",
            Self::Cron => "cron",
            Self::Tech => "tech",
            Self::Legacy => "legacy",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|h| h.as_str() == s.trim().to_lowercase())
    }

    /// Maps an MCP client's advertised name to a harness. Anything that is not
    /// the zeroclaw daemon reached the server as a plain MCP client.
    pub fn from_mcp_client(client_name: &str) -> Self {
        if client_name.trim().to_lowercase().contains("zeroclaw") {
            Self::Zeroclaw
        } else {
            Self::Mcp
        }
    }
}

impl fmt::Display for Harness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Placeholder actor for a stamp whose operator could not be determined.
pub const UNKNOWN_ACTOR: &str = "unknown";

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    pub harness: Harness,
    /// Agent alias or human id, already slugged.
    pub actor: String,
    /// Model that produced the work; `None` for a human.
    pub model: Option<String>,
    /// Host that ran it.
    pub node: Option<String>,
}

/// Lowercases and strips anything the grammar does not allow, so a caller
/// cannot smuggle a delimiter into a segment and change how it parses.
fn slug(raw: &str, extra_ok: &[char]) -> String {
    let cleaned: String = raw
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' || extra_ok.contains(&c)
            {
                c
            } else {
                '-'
            }
        })
        .collect();
    // Leading punctuation would fail the grammar's first-char rule.
    let trimmed = cleaned.trim_matches(|c: char| !c.is_ascii_alphanumeric());
    trimmed.to_string()
}

impl Provenance {
    pub fn new(harness: Harness, actor: &str) -> Self {
        let actor = slug(actor, &[]);
        Self {
            harness,
            actor: if actor.is_empty() { UNKNOWN_ACTOR.to_string() } else { actor },
            model: None,
            node: None,
        }
    }

    /// A human with no model involved.
    pub fn tech(actor: &str) -> Self {
        Self::new(Harness::Tech, actor)
    }

    pub fn with_model(mut self, model: impl AsRef<str>) -> Self {
        let m = slug(model.as_ref(), &[':']);
        self.model = (!m.is_empty()).then_some(m);
        self
    }

    pub fn with_node(mut self, node: impl AsRef<str>) -> Self {
        let n = slug(node.as_ref(), &[]);
        self.node = (!n.is_empty()).then_some(n);
        self
    }

    /// Fills the node from this host when the caller has not set one.
    pub fn with_local_node(self) -> Self {
        if self.node.is_some() {
            return self;
        }
        match hostname() {
            Some(h) => self.with_node(h),
            None => self,
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        let (harness, rest) = raw.split_once('/')?;
        let harness = Harness::parse(harness)?;

        // Split the optional tails off before reading the actor.
        let (head, node) = match rest.split_once('#') {
            Some((h, n)) => (h, Some(n.to_string())),
            None => (rest, None),
        };
        let (actor, model) = match head.split_once('@') {
            Some((a, m)) => (a, Some(m.to_string())),
            None => (head, None),
        };
        if actor.trim().is_empty() {
            return None;
        }
        let mut p = Self::new(harness, actor);
        if let Some(m) = model {
            p = p.with_model(m);
        }
        if let Some(n) = node {
            p = p.with_node(n);
        }
        Some(p)
    }

    /// True when the string is already in canonical form, so a DB ASSERT and
    /// this type agree on what is acceptable.
    pub fn is_canonical(raw: &str) -> bool {
        Self::parse(raw).map(|p| p.to_string() == raw.trim()) == Some(true)
    }
}

impl fmt::Display for Provenance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.harness, self.actor)?;
        if let Some(model) = &self.model {
            write!(f, "@{model}")?;
        }
        if let Some(node) = &self.node {
            write!(f, "#{node}")?;
        }
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn hostname() -> Option<String> {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .filter(|h| !h.trim().is_empty())
}

#[cfg(target_arch = "wasm32")]
fn hostname() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_every_documented_shape() {
        assert_eq!(
            Provenance::new(Harness::Mcp, "logan.lees")
                .with_model("claude-opus-5")
                .with_node("DESKTOP-EI5PV29")
                .to_string(),
            "mcp/logan.lees@claude-opus-5#desktop-ei5pv29"
        );
        assert_eq!(
            Provenance::new(Harness::Zeroclaw, "sweeper")
                .with_model("qwen3.8-unsloth:latest")
                .with_node("node-10")
                .to_string(),
            "zeroclaw/sweeper@qwen3.8-unsloth:latest#node-10"
        );
        assert_eq!(Provenance::tech("tyler.naylor").to_string(), "tech/tyler.naylor");
    }

    #[test]
    fn round_trips() {
        for raw in [
            "mcp/logan.lees@claude-opus-5#desktop-ei5pv29",
            "zeroclaw/sweeper@qwen3.8-unsloth:latest#node-10",
            "tech/tyler.naylor",
            "cron/bsod_sweep",
        ] {
            let parsed = Provenance::parse(raw).expect(raw);
            assert_eq!(parsed.to_string(), raw, "round trip for {raw}");
            assert!(Provenance::is_canonical(raw), "canonical for {raw}");
        }
    }

    #[test]
    fn rejects_the_free_text_that_caused_the_drift() {
        for raw in [
            "Claude (logan.lees)",
            "Logan (Claude-assisted)",
            "Sweeper (autopilot)",
            "zeroclaw:sweeper",
            "desktop",
            "",
            "/no-harness",
        ] {
            assert!(!Provenance::is_canonical(raw), "must reject {raw:?}");
        }
    }

    #[test]
    fn a_delimiter_inside_a_segment_cannot_change_parsing() {
        let p = Provenance::new(Harness::Mcp, "logan@evil#node").with_model("m");
        assert_eq!(p.actor, "logan-evil-node");
        assert_eq!(p.to_string(), "mcp/logan-evil-node@m");
        assert_eq!(Provenance::parse(&p.to_string()).unwrap().actor, "logan-evil-node");
    }

    #[test]
    fn empty_actor_becomes_unknown_rather_than_unparseable() {
        assert_eq!(Provenance::new(Harness::Legacy, "  ").to_string(), "legacy/unknown");
        assert_eq!(Provenance::new(Harness::Mcp, "!!!").actor, UNKNOWN_ACTOR);
    }

    #[test]
    fn client_name_decides_the_harness() {
        assert_eq!(Harness::from_mcp_client("zeroclaw"), Harness::Zeroclaw);
        assert_eq!(Harness::from_mcp_client("ZeroClaw-daemon"), Harness::Zeroclaw);
        assert_eq!(Harness::from_mcp_client("claude-code"), Harness::Mcp);
        assert_eq!(Harness::from_mcp_client("Claude Desktop"), Harness::Mcp);
    }
}
