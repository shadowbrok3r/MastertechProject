use std::fmt;

use compact_str::CompactString;
use serde::{Deserialize, Serialize};

/// Stable key for a script, independent of its display name and its category.
///
/// Slugs are `[a-z0-9-]` and at most a couple of dozen bytes, so `CompactString`
/// keeps them inline. This is the persisted vocabulary — renaming one orphans
/// every reference to it.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ScriptId(CompactString);

impl ScriptId {
    pub fn new(raw: impl Into<CompactString>) -> Self {
        Self(raw.into())
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// `[a-z0-9-]`, not empty, and no leading or trailing dash.
    pub fn is_wellformed(&self) -> bool {
        let s = self.as_str();
        !s.is_empty()
            && !s.starts_with('-')
            && !s.ends_with('-')
            && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    }
}

impl From<&str> for ScriptId {
    fn from(raw: &str) -> Self {
        Self::new(raw)
    }
}

impl AsRef<str> for ScriptId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for ScriptId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Debug for ScriptId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ScriptId({})", self.as_str())
    }
}
