//! Case-insensitive view of one model directory and the subdirectories its
//! recipes descend into, with lazily hashed files.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

/// Extensions that never hold firmware.
const DOC_EXT: [&str; 22] = [
    "txt", "doc", "docx", "pdf", "xls", "xlsx", "xlsm", "jpg", "jpeg", "png", "gif", "bmp", "db",
    "log", "ini", "zip", "rar", "7z", "cer", "crt", "html", "csv",
];

/// Extensions that are tooling rather than payload.
const TOOL_EXT: [&str; 7] = ["efi", "exe", "com", "dll", "sys", "nsh", "bat"];

/// Subdirectory levels below the model folder that are indexed.
const MAX_DEPTH: usize = 2;

#[derive(Debug, Clone)]
pub struct FileMeta {
    /// Path relative to the model folder, `/`-separated, as spelled on disk.
    pub name: String,
    pub size: u64,
}

impl FileMeta {
    /// Filename without its directory.
    pub fn base(&self) -> &str {
        self.name.rsplit_once('/').map_or(self.name.as_str(), |(_, b)| b)
    }

    pub fn ext(&self) -> &str {
        self.base().rsplit_once('.').map_or("", |(_, e)| e)
    }

    /// Top level of the model folder rather than a subdirectory.
    pub fn is_top(&self) -> bool {
        !self.name.contains('/')
    }
}

pub struct DirIndex {
    root: PathBuf,
    files: HashMap<String, FileMeta>,
    /// Lowercase relative path to the directory as spelled on disk.
    dirs: HashMap<String, String>,
    hashes: RefCell<HashMap<String, String>>,
}

/// Candidate keys for `name` seen from `cwd`, nearest directory first.
fn keys_from(cwd: &str, name: &str) -> Vec<String> {
    let name = name.replace('\\', "/");
    let name = name.trim_start_matches('/').to_ascii_lowercase();
    let mut out = Vec::new();
    let mut base = cwd;
    loop {
        if base.is_empty() {
            out.push(name.clone());
            return out;
        }
        out.push(format!("{}/{name}", base.to_ascii_lowercase()));
        base = base.rsplit_once('/').map_or("", |(parent, _)| parent);
    }
}

impl DirIndex {
    pub fn read(root: &Path) -> Result<Self> {
        let mut index = Self {
            root: root.to_path_buf(),
            files: HashMap::new(),
            dirs: HashMap::new(),
            hashes: RefCell::new(HashMap::new()),
        };
        let rd = std::fs::read_dir(root).with_context(|| format!("read {}", root.display()))?;
        index.absorb(rd, "", 0);
        Ok(index)
    }

    fn absorb(&mut self, rd: std::fs::ReadDir, rel: &str, depth: usize) {
        let mut subdirs = Vec::new();
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_file() {
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                self.files
                    .insert(path.to_ascii_lowercase(), FileMeta { name: path, size });
            } else if kind.is_dir() && depth < MAX_DEPTH {
                self.dirs.insert(path.to_ascii_lowercase(), path.clone());
                subdirs.push(path);
            }
        }
        for sub in subdirs {
            if let Ok(rd) = std::fs::read_dir(self.root.join(&sub)) {
                self.absorb(rd, &sub, depth + 1);
            }
        }
    }

    /// Files at the top level of the model folder.
    pub fn iter(&self) -> impl Iterator<Item = &FileMeta> {
        self.files.values().filter(|f| f.is_top())
    }

    /// Look `name` up from `cwd`, then from each directory above it.
    pub fn get_in(&self, cwd: &str, name: &str) -> Option<&FileMeta> {
        keys_from(cwd, name).iter().find_map(|k| self.files.get(k))
    }

    pub fn get(&self, name: &str) -> Option<&FileMeta> {
        self.get_in("", name)
    }

    /// Subdirectory `name` seen from `cwd`, as its path relative to the folder.
    pub fn subdir_in(&self, cwd: &str, name: &str) -> Option<&str> {
        keys_from(cwd, name)
            .iter()
            .find_map(|k| self.dirs.get(k))
            .map(String::as_str)
    }

    /// A bare tool name resolves through the extensions a shell would try.
    pub fn resolve_exec_in(&self, cwd: &str, name: &str, prefer_efi: bool) -> Option<&FileMeta> {
        if let Some(m) = self.get_in(cwd, name) {
            return Some(m);
        }
        if name.contains('.') {
            return None;
        }
        let order: [&str; 3] = if prefer_efi {
            [".efi", ".exe", ".com"]
        } else {
            [".exe", ".com", ".efi"]
        };
        order
            .iter()
            .find_map(|ext| self.get_in(cwd, &format!("{name}{ext}")))
    }

    /// Firmware image with no launcher: big, top level, neither document nor tool.
    pub fn bare_payloads(&self) -> Vec<&FileMeta> {
        let mut out: Vec<&FileMeta> = self
            .files
            .values()
            .filter(|f| f.is_top() && f.size >= 128 * 1024)
            .filter(|f| {
                let ext = f.ext().to_ascii_lowercase();
                !DOC_EXT.contains(&ext.as_str()) && !TOOL_EXT.contains(&ext.as_str())
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// True when the folder carries a program of its own.
    pub fn has_executable(&self) -> bool {
        self.iter()
            .any(|f| matches!(f.ext().to_ascii_lowercase().as_str(), "efi" | "exe" | "com"))
    }

    pub fn read_text(&self, name: &str) -> Option<String> {
        let meta = self.get(name)?;
        let bytes = std::fs::read(self.root.join(&meta.name)).ok()?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }

    pub fn sha256(&self, name: &str) -> Result<String> {
        let key = name.to_ascii_lowercase();
        if let Some(hex) = self.hashes.borrow().get(&key) {
            return Ok(hex.clone());
        }
        let meta = self
            .files
            .get(&key)
            .with_context(|| format!("{name} is not in {}", self.root.display()))?;
        let path = self.root.join(&meta.name);
        let mut file = File::open(&path).with_context(|| format!("open {}", path.display()))?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 1024 * 1024];
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        let hex = format!("{:x}", hasher.finalize());
        self.hashes.borrow_mut().insert(key, hex.clone());
        Ok(hex)
    }

    /// First bytes of a file, for header sniffing.
    pub fn head(&self, name: &str, len: usize) -> Option<Vec<u8>> {
        let meta = self.get(name)?;
        let mut file = File::open(self.root.join(&meta.name)).ok()?;
        let mut buf = vec![0u8; len];
        let mut filled = 0;
        while filled < len {
            match file.read(&mut buf[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(_) => return None,
            }
        }
        buf.truncate(filled);
        Some(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_walks_up_from_the_working_directory() {
        assert_eq!(keys_from("", "flash.nsh"), ["flash.nsh"]);
        assert_eq!(keys_from("68C1", "Flash.nsh"), ["68c1/flash.nsh", "flash.nsh"]);
        assert_eq!(
            keys_from("a/B", "t.efi"),
            ["a/b/t.efi", "a/t.efi", "t.efi"]
        );
        assert_eq!(keys_from("", r"MTL-H\Flash.nsh"), ["mtl-h/flash.nsh"]);
    }

    #[test]
    fn relative_paths_split_into_directory_and_name() {
        let f = FileMeta {
            name: "MTL-H/V5xxTU15m.efi".to_string(),
            size: 0,
        };
        assert_eq!(f.base(), "V5xxTU15m.efi");
        assert_eq!(f.ext(), "efi");
        assert!(!f.is_top());
    }
}
