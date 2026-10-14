//! Applying and reverting patches.
//!
//! targets under /opt or /usr are root-owned, so the user should re-run the program as
//! `sudo -E`. no polkit and such for compatibility here. Restore writes only the canonical
//! 0.2 / 1.2 values and refuses unless the result hashes to the recorded pristine hash;
//! the offsets always come from our own scan

use crate::elf::{self, Pair};
use crate::store::{Entry, Store};
use std::path::{Path, PathBuf};

pub const DEFAULT_GAMMA: f32 = 1.8;
pub const DEFAULT_CONTRAST: f32 = 0.2;

/// How a patched binary can be brought back.
#[derive(Clone, Debug)]
pub enum RestorePlan {
    /// Offsets and pristine hash are recorded: un-patch in place, then verify.
    Restore { orig_hash: String, pairs: Vec<Pair> },
    /// No record (foreign patch, lost state): copy a pristine backup over it.
    RestoreBackup { backup: PathBuf },
}

impl RestorePlan {
    pub fn label(&self) -> String {
        match self {
            RestorePlan::Restore { pairs, .. } => format!("restore {} pair(s) in place", pairs.len()),
            RestorePlan::RestoreBackup { backup } => {
                format!("restore from backup {}", backup.to_string_lossy())
            }
        }
    }
}

pub struct Outcome {
    pub detail: String,
    pub orig_hash: String,
    pub patched_hash: String,
    pub pairs: Vec<Pair>,
    pub backup: Option<PathBuf>,
    pub gamma: f32,
    pub contrast: f32,
}

pub fn self_exe() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("chromium-gamma-patcher"))
}

/// The exact instruction to give the user when a target needs root.
pub fn sudo_hint(_path: &Path) -> String {
    format!("run:  sudo -E {}", self_exe().display())
}

/// Current user name, for messages
pub fn user_name() -> String {
    std::env::var("SUDO_USER")
        .ok()
        .filter(|s| !s.is_empty() && s != "root")
        .or_else(|| std::env::var("USER").ok())
        .unwrap_or_else(|| "this user".to_string())
}

/// True when we can write the binary *and* create a backup next to it.
///
/// Note this is false for a binary that is currently running (`ETXTBSY`), which
/// is not a privilege problem: a live executable cannot be rewritten in place by
/// anyone, root included.
pub fn is_writable(path: &Path) -> bool {
    elf::write_block(path).is_none() && (elf::is_root() || elf::can_write_dir(path))
}

/// A message the user can act on, or `None` when the write can go ahead.
pub fn write_blocker_message(path: &Path) -> Option<String> {
    match elf::write_block(path) {
        None => {
            if elf::is_root() || elf::can_write_dir(path) {
                None
            } else {
                Some(format!(
                    "cannot create a backup beside {} - {}",
                    path.display(),
                    sudo_hint(path)
                ))
            }
        }
        Some(elf::WriteBlock::Busy) => Some(format!(
            "{} is running - quit it first",
            path.display()
        )),
        Some(elf::WriteBlock::Denied) => Some(format!(
            "{} needs root - {}",
            path.display(),
            sudo_hint(path)
        )),
        Some(elf::WriteBlock::Other(e)) => Some(format!("cannot write {}: {e}", path.display())),
    }
}

fn needs_root_message(path: &Path) -> String {
    write_blocker_message(path).unwrap_or_else(|| format!("cannot write {}", path.display()))
}

/// Patch a pristine binary. Returns the hashes needed to record the operation.
pub fn do_patch(
    path: &Path,
    gamma: f32,
    contrast: f32,
    make_backup: bool,
) -> Result<Outcome, String> {
    if let Some(msg) = write_blocker_message(path) {
        return Err(msg);
    }
    let data = elf::read_file(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let pairs = elf::find_pairs(&data);
    if pairs.is_empty() {
        return Err(format!(
            "no (0.2, 1.2) signature in {} - already patched, or a build we do not recognise",
            path.display()
        ));
    }
    let orig_hash = elf::sha256_hex(&data);
    let backup = elf::backup_path_for(path, &orig_hash);

    let mut kept_backup = None;
    if make_backup {
        if !backup.exists() {
            std::fs::copy(path, &backup)
                .map_err(|e| format!("backup {}: {e}", backup.display()))?;
        }
        kept_backup = Some(backup.clone());
    }

    // NB: patch_in_place takes (contrast, gamma) in that order - the 0.2f
    // immediate is the contrast and the 1.2f one is the gamma
    elf::patch_in_place(path, &pairs, contrast, gamma).map_err(|e| format!("write: {e}"))?;
    let patched_hash = elf::sha256_file(path).map_err(|e| format!("hash: {e}"))?;

    let mut detail = format!(
        "{} pair(s) patched: contrast 0.2 -> {contrast}, gamma 1.2 -> {gamma}",
        pairs.len()
    );
    if let Some(b) = &kept_backup {
        detail.push_str(&format!("; backup {}", b.to_string_lossy()));
    }
    Ok(Outcome {
        detail,
        orig_hash,
        patched_hash,
        pairs,
        backup: kept_backup,
        gamma,
        contrast,
    })
}

/// Restore in place, hash-verified (no backup copy needed).
pub fn do_restore(path: &Path, orig_hash: &str, pairs: &[Pair]) -> Result<String, String> {
    if let Some(msg) = write_blocker_message(path) {
        return Err(msg);
    }
    elf::restore_verified(path, pairs, orig_hash)?;
    Ok(format!(
        "{} pair(s) restored to contrast 0.2 / gamma 1.2 (verified against {})",
        pairs.len(),
        &orig_hash[..12.min(orig_hash.len())]
    ))
}

/// Restore by copying a pristine backup over the binary, after checking that the
/// backup really carries the signature
pub fn do_restore_backup(path: &Path, backup: &Path) -> Result<String, String> {
    if let Some(msg) = write_blocker_message(path) {
        return Err(msg);
    }
    let data = elf::read_file(backup).map_err(|e| format!("read backup: {e}"))?;
    if elf::find_pairs(&data).is_empty() {
        return Err(format!(
            "{} does not carry the (0.2, 1.2) signature - refusing to restore",
            backup.display()
        ));
    }
    std::fs::copy(backup, path).map_err(|e| format!("copy: {e}"))?;
    Ok(format!("restored {} from {}", path.display(), backup.display()))
}

/// Which restore strategy applies to `path` right now (used by GUI and CLI).
pub fn restore_plan(path: &Path, store: &Store) -> Result<RestorePlan, String> {
    let hash = elf::sha256_file(path).map_err(|e| format!("hash: {e}"))?;
    if let Some(entry) = store.find(path, &hash) {
        if entry.patched_hash == hash {
            return Ok(RestorePlan::Restore {
                orig_hash: entry.orig_hash.clone(),
                pairs: entry.pairs.clone(),
            });
        }
    }
    // Fall back to a pristine backup that still carries the signature.
    for b in elf::sibling_backups(path) {
        if let Ok(data) = elf::read_file(&b) {
            if !elf::find_pairs(&data).is_empty() {
                return Ok(RestorePlan::RestoreBackup { backup: b });
            }
        }
    }
    Err(format!(
        "cannot restore {}: no recorded offsets and no pristine backup carrying the signature - reinstall the browser",
        path.display()
    ))
}

/// Record a successful patch so a later run can restore in place without a
/// backup copy being present.
pub fn record_patch(store: &mut Store, path: &Path, o: &Outcome) {
    store.record(Entry {
        path: path.to_path_buf(),
        orig_hash: o.orig_hash.clone(),
        patched_hash: o.patched_hash.clone(),
        pairs: o.pairs.clone(),
        gamma: o.gamma,
        contrast: o.contrast,
        backup: o.backup.clone(),
        ts: now_ts(),
    });
}

pub fn now_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// CLI verbs - can use without a desktop session
// ---------------------------------------------------------------------------

/// `patch <path> [gamma] [contrast]`
pub fn cli_patch(args: &[String]) -> i32 {
    let Some(raw) = args.first() else {
        eprintln!("usage: patch <path> [gamma] [contrast]");
        return 2;
    };
    let path = crate::discovery::expand_tilde(raw);
    if !path.is_file() {
        eprintln!("not a file: {}", path.display());
        return 2;
    }
    if !is_writable(&path) {
        eprintln!("{}", needs_root_message(&path));
        return 3;
    }
    let gamma = args
        .get(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_GAMMA);
    let contrast = args
        .get(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_CONTRAST);

    let mut store = Store::load();
    match do_patch(&path, gamma, contrast, true) {
        Ok(o) => {
            record_patch(&mut store, &path, &o);
            println!("OK {}", o.detail);
            println!(
                "orig={} patched={} pairs={}",
                o.orig_hash,
                o.patched_hash,
                o.pairs.iter().map(|p| p.encode()).collect::<Vec<_>>().join(",")
            );
            if let Some(b) = &o.backup {
                println!("backup={}", b.to_string_lossy());
            }
            0
        }
        Err(e) => {
            eprintln!("FAIL {e}");
            1
        }
    }
}

/// `restore <path>`
pub fn cli_restore(args: &[String]) -> i32 {
    let Some(raw) = args.first() else {
        eprintln!("usage: restore <path>");
        return 2;
    };
    let path = crate::discovery::expand_tilde(raw);
    if !path.is_file() {
        eprintln!("not a file: {}", path.display());
        return 2;
    }
    if !is_writable(&path) {
        eprintln!("{}", needs_root_message(&path));
        return 3;
    }
    let store = Store::load();
    let op = match restore_plan(&path, &store) {
        Ok(op) => op,
        Err(e) => {
            eprintln!("FAIL {e}");
            return 1;
        }
    };
    let result = match &op {
        RestorePlan::Restore { orig_hash, pairs } => do_restore(&path, orig_hash, pairs),
        RestorePlan::RestoreBackup { backup } => do_restore_backup(&path, backup),
    };
    match result {
        Ok(detail) => {
            println!("OK {detail}");
            let mut store = store;
            store.entries.retain(|e| e.path != path);
            let _ = store.save();
            0
        }
        Err(e) => {
            eprintln!("FAIL {e}");
            1
        }
    }
}