//! Scanning: read a binary, find the signature, work out its state. Runs on a
//! small worker pool so the UI thread never blocks on a big file read

use crate::discovery;
use crate::elf::{self, Pair};
use crate::store::Store;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowState {
    /// Signature present: original values, patchable.
    Pristine,
    /// Patched by this tool: offsets and pristine hash are recorded.
    PatchedKnown,
    /// Patched by something else: no signature, no record, but a pristine
    /// backup sits next to it.
    PatchedUnknown,
    /// No signature and no record of us.
    Unsigned,
    /// Could not be read.
    Unreadable,
}

#[derive(Clone, Debug)]
pub struct Row {
    pub path: PathBuf,
    pub label: String,
    pub size: u64,
    pub hash: String,
    pub pairs: Vec<Pair>,
    pub state: RowState,
    pub bookmarked: bool,
    pub running_pids: Vec<u32>,
    pub backups: Vec<PathBuf>,
    pub writable: bool,
    /// Running right now, so the kernel refuses writes (`ETXTBSY`).
    pub busy: bool,
    /// Values we previously applied (only for `PatchedKnown`).
    pub applied: Option<(f32, f32)>,
    pub error: Option<String>,
}

impl Row {
    pub fn state_text(&self) -> String {
        match self.state {
            RowState::Pristine => format!("pristine - {} pair(s) located", self.pairs.len()),
            RowState::PatchedKnown => match self.applied {
                Some((g, c)) => format!("patched by this tool - gamma {g}, contrast {c}"),
                None => "patched by this tool".to_string(),
            },
            RowState::PatchedUnknown => format!(
                "patched, offsets unknown - {} pristine backup(s) available",
                self.backups.len()
            ),
            RowState::Unsigned => "no signature - not a match".to_string(),
            RowState::Unreadable => format!(
                "unreadable: {}",
                self.error.as_deref().unwrap_or("permission denied")
            ),
        }
    }

    pub fn can_patch(&self) -> bool {
        self.state == RowState::Pristine
    }

    pub fn can_restore(&self) -> bool {
        self.state == RowState::PatchedKnown || self.state == RowState::PatchedUnknown
    }
}

pub enum ScanMsg {
    Progress { done: usize, total: usize, path: String },
    Row(Row),
    Finished { scanned: usize, listed: usize },
}

/// Scan one file (used both by the pool and for single-file refreshes).
pub fn scan_one(path: &Path, bookmarked: bool, store: &Store) -> Row {
    let mut row = Row {
        path: path.to_path_buf(),
        label: discovery::label_for(path),
        size: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
        hash: String::new(),
        pairs: Vec::new(),
        state: RowState::Unsigned,
        bookmarked,
        running_pids: discovery::running_pids_for(path),
        backups: elf::sibling_backups(path),
        writable: crate::apply::is_writable(path),
        busy: matches!(elf::write_block(path), Some(elf::WriteBlock::Busy)),
        applied: None,
        error: None,
    };

    match elf::read_file(path) {
        Ok(data) => {
            row.size = data.len() as u64;
            row.hash = elf::sha256_hex(&data);
            row.pairs = elf::find_pairs(&data);
            if !row.pairs.is_empty() {
                // Signature intact: the original values are still in place.
                row.state = RowState::Pristine;
            } else if let Some(entry) = store.find(path, &row.hash) {
                // Either we patched it (hash == patched_hash) or it is the
                // pristine image of something we recorded (hash == orig_hash),
                // in which case the signature scan should have hit.
                row.pairs = entry.pairs.clone();
                row.state = RowState::PatchedKnown;
                row.applied = Some((entry.gamma, entry.contrast));
            } else if !row.backups.is_empty() {
                // Patched by something else, but a pristine backup exists.
                row.state = RowState::PatchedUnknown;
            } else {
                row.state = RowState::Unsigned;
            }
        }
        Err(e) => {
            row.state = RowState::Unreadable;
            row.error = Some(e.to_string());
        }
    }
    row
}

fn worth_listing(row: &Row) -> bool {
    matches!(
        row.state,
        RowState::Pristine | RowState::PatchedKnown | RowState::PatchedUnknown | RowState::Unreadable
    ) || row.bookmarked
}

/// Spawn the worker pool. Returns immediately; results arrive on `tx`.
pub fn spawn_scan(paths: Vec<(PathBuf, bool)>, store: Arc<Store>, tx: Sender<ScanMsg>) {
    std::thread::spawn(move || {
        let threads = std::thread::available_parallelism()
            .map(|n| n.get().min(8))
            .unwrap_or(4);

        let paths = Arc::new(paths);
        let next = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (done_tx, done_rx) = std::sync::mpsc::channel::<Row>();

        let mut handles = Vec::new();
        for _ in 0..threads {
            let paths = paths.clone();
            let next = next.clone();
            let store = store.clone();
            let tx = tx.clone();
            let done_tx = done_tx.clone();
            handles.push(std::thread::spawn(move || loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if i >= paths.len() {
                    break;
                }
                let (path, bookmarked) = &paths[i];
                let row = scan_one(path, *bookmarked, &store);
                let _ = tx.send(ScanMsg::Progress {
                    done: i + 1,
                    total: paths.len(),
                    path: path.to_string_lossy().to_string(),
                });
                let _ = done_tx.send(row);
            }));
        }
        drop(done_tx);

        let mut scanned = 0;
        let mut listed = 0;
        for row in done_rx.iter() {
            scanned += 1;
            if worth_listing(&row) {
                listed += 1;
                let _ = tx.send(ScanMsg::Row(row));
            }
        }
        for h in handles {
            let _ = h.join();
        }
        let _ = tx.send(ScanMsg::Finished { scanned, listed });
    });
}
