//! Persisted state: pinned paths and the record of what was patched.
//!
//! `bookmarks.txt` is one path per line, `state.tsv` one patch per line keyed by
//! the pristine and patched sha256 - enough to identify a patched binary and
//! un-patch it without a backup copy. Both live in
//! `$XDG_CONFIG_HOME/chromium-gamma-patcher/`.

use crate::elf::Pair;
use std::path::{Path, PathBuf};

/// its fine what like your software doesnt read sketchy files?
/// fr this is not /etc/shadow its just to get the home dir
fn home_for(user: &str) -> Option<PathBuf> {
    let passwd = std::fs::read_to_string("/etc/passwd").ok()?;
    for line in passwd.lines() {
        let f: Vec<&str> = line.split(':').collect();
        if f.len() >= 6 && f[0] == user && !f[5].is_empty() {
            return Some(PathBuf::from(f[5]));
        }
    }
    None
}

/// When the program is started through `sudo`, `$HOME` points at root's home
/// and the user would "lose" their bookmarks between runs. `sudo -E` keeps
/// `SUDO_USER`, so use the invoking user's config directory in that case.
pub fn config_dir() -> PathBuf {
    let sudo_user = std::env::var("SUDO_USER")
        .ok()
        .filter(|u| !u.is_empty() && u != "root");

    let base = sudo_user
        .as_deref()
        .and_then(home_for)
        .map(|h| h.join(".config"))
        .or_else(|| {
            std::env::var("XDG_CONFIG_HOME")
                .ok()
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
        })
        .or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("chromium-gamma-patcher")
}

fn now_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `sudo -E` runs would otherwise leave root-owned files in the user's config
/// dir, which then break pinning from a normal run. Hand them to the invoker.
fn chown_to_invoker(path: &Path) {
    let uid = std::env::var("SUDO_UID").ok().and_then(|v| v.parse().ok());
    let gid = std::env::var("SUDO_GID").ok().and_then(|v| v.parse().ok());
    let _ = std::os::unix::fs::chown(path, uid, gid);
}

/// One applied patch.
#[derive(Clone, Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub orig_hash: String,
    pub patched_hash: String,
    pub pairs: Vec<Pair>,
    pub gamma: f32,
    pub contrast: f32,
    pub backup: Option<PathBuf>,
    pub ts: u64,
}

#[derive(Default, Clone)]
pub struct Store {
    pub bookmarks: Vec<PathBuf>,
    pub entries: Vec<Entry>,
    pub loaded: bool,
}

impl Store {
    pub fn load() -> Store {
        let dir = config_dir();
        let mut s = Store::default();

        if let Ok(text) = std::fs::read_to_string(dir.join("bookmarks.txt")) {
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let p = crate::discovery::expand_tilde(line);
                if !s.bookmarks.contains(&p) {
                    s.bookmarks.push(p);
                }
            }
        }

        if let Ok(text) = std::fs::read_to_string(dir.join("state.tsv")) {
            for line in text.lines() {
                if let Some(e) = Entry::decode(line) {
                    s.entries.push(e);
                }
            }
        }
        s.loaded = true;
        s
    }

    fn save_bookmarks(&self) -> std::io::Result<()> {
        let dir = config_dir();
        std::fs::create_dir_all(&dir)?;
        chown_to_invoker(&dir);
        let mut out = String::from("# chromium-gamma-patcher bookmarks - one path per line\n");
        for b in &self.bookmarks {
            out.push_str(&b.to_string_lossy());
            out.push('\n');
        }
        let file = dir.join("bookmarks.txt");
        std::fs::write(&file, out)?;
        chown_to_invoker(&file);
        Ok(())
    }

    fn save_entries(&self) -> std::io::Result<()> {
        let dir = config_dir();
        std::fs::create_dir_all(&dir)?;
        chown_to_invoker(&dir);
        let mut out = String::from("# path\torig_hash\tpatched_hash\tpairs\tgamma\tcontrast\tbackup\tts\n");
        for e in &self.entries {
            out.push_str(&e.encode());
            out.push('\n');
        }
        let file = dir.join("state.tsv");
        std::fs::write(&file, out)?;
        chown_to_invoker(&file);
        Ok(())
    }

    pub fn save(&self) -> std::io::Result<()> {
        self.save_bookmarks()?;
        self.save_entries()
    }

    pub fn add_bookmark(&mut self, path: &Path) -> std::io::Result<bool> {
        if self.bookmarks.iter().any(|p| p == path) {
            return Ok(false);
        }
        self.bookmarks.push(path.to_path_buf());
        self.save_bookmarks().map(|_| true)
    }

    pub fn remove_bookmark(&mut self, path: &Path) -> std::io::Result<bool> {
        let before = self.bookmarks.len();
        self.bookmarks.retain(|p| p != path);
        if self.bookmarks.len() == before {
            return Ok(false);
        }
        self.save_bookmarks().map(|_| true)
    }

    pub fn is_bookmarked(&self, path: &Path) -> bool {
        self.bookmarks.iter().any(|p| p == path)
    }

    /// The most recent record for `path` matching `hash` as either the pristine
    /// or the patched image.
    pub fn find(&self, path: &Path, hash: &str) -> Option<&Entry> {
        self.entries
            .iter()
            .rev()
            .find(|e| e.path == path && (e.orig_hash == hash || e.patched_hash == hash))
    }

    pub fn record(&mut self, entry: Entry) {
        // Keep the store small: the last 20 entries per path.
        let path = entry.path.clone();
        self.entries.push(entry);
        let mut seen = 0;
        let mut keep: Vec<Entry> = Vec::with_capacity(self.entries.len());
        for e in self.entries.drain(..).rev() {
            if e.path == path {
                seen += 1;
                if seen > 20 {
                    continue;
                }
            }
            keep.push(e);
        }
        keep.reverse();
        self.entries = keep;
        let _ = self.save_entries();
    }
}

impl Entry {
    fn encode(&self) -> String {
        let pairs = self
            .pairs
            .iter()
            .map(|p| p.encode())
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            self.path.to_string_lossy(),
            self.orig_hash,
            self.patched_hash,
            pairs,
            self.gamma,
            self.contrast,
            self.backup.as_ref().map(|b| b.to_string_lossy().to_string()).unwrap_or_default(),
            self.ts
        )
    }

    fn decode(line: &str) -> Option<Entry> {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 8 || f[0].starts_with('#') || f[0].is_empty() {
            return None;
        }
        let pairs = f[3]
            .split(',')
            .filter(|s| !s.is_empty())
            .filter_map(Pair::decode)
            .collect::<Vec<_>>();
        if pairs.is_empty() {
            return None;
        }
        Some(Entry {
            path: PathBuf::from(f[0]),
            orig_hash: f[1].to_string(),
            patched_hash: f[2].to_string(),
            pairs,
            gamma: f[4].parse().ok()?,
            contrast: f[5].parse().ok()?,
            backup: if f[6].is_empty() { None } else { Some(PathBuf::from(f[6])) },
            ts: f[7].parse().unwrap_or_else(|_| now_ts()),
        })
    }
}
