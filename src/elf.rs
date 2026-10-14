//! ELF parsing and the font-gamma signature.
//!
//! The patch target is the (contrast, gamma) float pair Chromium's font code
//! returns: 0.2f (CD CC 4C 3E) followed 8..16 bytes later by 1.2f (9A 99 99 3F),
//! inside an executable LOAD segment. Code immediates rather than a data table,
//! so the layout holds across Chromium and Electron versions.

use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub const CONTRAST_ORIG: f32 = 0.2;
pub const GAMMA_ORIG: f32 = 1.2;

/// 0.2f32 = 0x3E4CCCCD
pub const SIG_CONTRAST: [u8; 4] = [0xcd, 0xcc, 0x4c, 0x3e];
/// 1.2f32 = 0x3F99999A
pub const SIG_GAMMA: [u8; 4] = [0x9a, 0x99, 0x99, 0x3f];

/// Byte distance between the two immediates so we don't match random garbage
const GAP_MIN: usize = 8;
const GAP_MAX: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pair {
    pub contrast_off: u64,
    pub gamma_off: u64,
}

impl Pair {
    pub fn encode(&self) -> String {
        format!("{}:{}", self.contrast_off, self.gamma_off)
    }
    pub fn decode(s: &str) -> Option<Pair> {
        let (a, b) = s.split_once(':')?;
        Some(Pair {
            contrast_off: a.parse().ok()?,
            gamma_off: b.parse().ok()?,
        })
    }
}

pub fn is_elf(data: &[u8]) -> bool {
    data.len() > 64 && &data[0..4] == b"\x7fELF"
}

/// File-offset ranges of PT_LOAD segments carrying the execute flag. Returns
/// None when the image is not a little-endian ELF64 (we only support what
/// Chromium-family binaries ship as).
pub fn exec_ranges(data: &[u8]) -> Option<Vec<(u64, u64)>> {
    if !is_elf(data) || data[4] != 2 || data[5] != 1 {
        return None;
    }
    let get16 = |o: usize| u16::from_le_bytes([data[o], data[o + 1]]) as usize;
    let get32 = |o: usize| u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
    let get64 = |o: usize| {
        u64::from_le_bytes([
            data[o],
            data[o + 1],
            data[o + 2],
            data[o + 3],
            data[o + 4],
            data[o + 5],
            data[o + 6],
            data[o + 7],
        ])
    };
    let phoff = get64(0x20) as usize;
    let phentsize = get16(0x36);
    let phnum = get16(0x38);
    if phoff == 0 || phentsize < 56 || phnum == 0 {
        return None;
    }
    let mut out = Vec::new();
    for i in 0..phnum {
        let off = phoff + i * phentsize;
        if off + phentsize > data.len() || off + 56 > data.len() {
            break;
        }
        let p_type = get32(off);
        let p_flags = get32(off + 4);
        let p_offset = get64(off + 8);
        let p_filesz = get64(off + 0x20);
        // PT_LOAD = 1, PF_X = 1
        if p_type == 1 && p_flags & 1 != 0 {
            out.push((p_offset, p_offset + p_filesz));
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// First-byte-skip search for a 4 byte needle. Keeps the whole pass branch
/// predictable and runs at memory bandwidth.
fn find_all(hay: &[u8], needle: [u8; 4]) -> Vec<u64> {
    let mut out = Vec::new();
    let n = hay.len();
    if n < 4 {
        return out;
    }
    let mut i = 0usize;
    while i + 4 <= n {
        if hay[i] == needle[0] && hay[i + 1] == needle[1] && hay[i + 2] == needle[2] && hay[i + 3] == needle[3] {
            out.push(i as u64);
            i += 4;
        } else {
            i += 1;
        }
    }
    out
}

fn in_ranges(ranges: &[(u64, u64)], a: u64, b: u64) -> bool {
    ranges.iter().any(|&(t0, t1)| t0 <= a && b < t1)
}

/// Locate every (0.2f, 1.2f) immediate pair inside the executable segments.
pub fn find_pairs(data: &[u8]) -> Vec<Pair> {
    let ranges = match exec_ranges(data) {
        Some(r) => r,
        None => return Vec::new(),
    };
    let mut out = Vec::new();
    for i in find_all(data, SIG_CONTRAST) {
        let iu = i as usize;
        for d in GAP_MIN..=GAP_MAX {
            let j = iu + d;
            if j + 4 > data.len() {
                break;
            }
            if data[j..j + 4] == SIG_GAMMA {
                if in_ranges(&ranges, i, (j + 4) as u64) {
                    out.push(Pair {
                        contrast_off: i,
                        gamma_off: j as u64,
                    });
                }
                break;
            }
        }
    }
    out
}

pub fn read_f32_at(data: &[u8], off: u64) -> Option<f32> {
    let o = off as usize;
    if o + 4 > data.len() {
        return None;
    }
    Some(f32::from_le_bytes([
        data[o],
        data[o + 1],
        data[o + 2],
        data[o + 3],
    ]))
}

pub fn read_file(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut f = File::open(path)?;
    let len = f.metadata()?.len() as usize;
    let mut buf = vec![0u8; len];
    f.read_exact(&mut buf)?;
    Ok(buf)
}

pub fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    format!("{:x}", h.finalize())
}

pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut f = File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}

fn write_f32(f: &mut File, off: u64, v: f32) -> std::io::Result<()> {
    f.seek(SeekFrom::Start(off))?;
    f.write_all(&v.to_le_bytes())
}

/// Write new immediates at every located pair. `pairs` must come from
/// `find_pairs` (or from this tool's own state store, validated below).
pub fn patch_in_place(
    path: &Path,
    pairs: &[Pair],
    contrast: f32,
    gamma: f32,
) -> std::io::Result<()> {
    let mut f = OpenOptions::new().write(true).open(path)?;
    for p in pairs {
        write_f32(&mut f, p.contrast_off, contrast)?;
        write_f32(&mut f, p.gamma_off, gamma)?;
    }
    f.sync_all()?;
    Ok(())
}

/// Restore the canonical (0.2, 1.2) immediates.
pub fn restore_in_place(path: &Path, pairs: &[Pair]) -> std::io::Result<()> {
    patch_in_place(path, pairs, CONTRAST_ORIG, GAMMA_ORIG)
}

/// Restore in place, but only if the result is bit-identical to the recorded
/// pristine image (`orig_hash`). This makes it harder to be stupid:
/// the offsets are validated by hashing the reconstructed file.
pub fn restore_verified(
    path: &Path,
    pairs: &[Pair],
    orig_hash: &str,
) -> Result<(), String> {
    let mut data = read_file(path).map_err(|e| e.to_string())?;
    for p in pairs {
        let (co, go) = (p.contrast_off as usize, p.gamma_off as usize);
        if co + 4 > data.len() || go + 4 > data.len() {
            return Err(format!("pair {p:?} outside file"));
        }
        data[co..co + 4].copy_from_slice(&CONTRAST_ORIG.to_le_bytes());
        data[go..go + 4].copy_from_slice(&GAMMA_ORIG.to_le_bytes());
    }
    let got = sha256_hex(&data);
    if !got.starts_with(orig_hash) && got != orig_hash {
        return Err(format!(
            "refusing to restore: reconstructed hash {} != recorded original {}",
            &got[..12.min(got.len())],
            &orig_hash[..12.min(orig_hash.len())]
        ));
    }
    let mut f = OpenOptions::new().write(true).open(path).map_err(|e| e.to_string())?;
    for p in pairs {
        write_f32(&mut f, p.contrast_off, CONTRAST_ORIG).map_err(|e| e.to_string())?;
        write_f32(&mut f, p.gamma_off, GAMMA_ORIG).map_err(|e| e.to_string())?;
    }
    f.sync_all().map_err(|e| e.to_string())?;
    Ok(())
}

/// Backup path for a binary, keyed by the pristine image hash
pub fn backup_path_for(path: &Path, pristine_hash: &str) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(format!(".gamma.bak-{}", &pristine_hash[..12.min(pristine_hash.len())]));
    PathBuf::from(s)
}

/// All sibling backups of `path`.
pub fn sibling_backups(path: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let (dir, name) = match (path.parent(), path.file_name()) {
        (Some(d), Some(n)) => (d, n.to_string_lossy().to_string()),
        _ => return out,
    };
    let prefix = format!("{name}.gamma.bak-");
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if n.starts_with(&prefix) {
                out.push(e.path());
            }
        }
    }
    out.sort();
    out
}

pub fn can_write_file(path: &Path) -> bool {
    OpenOptions::new().write(true).open(path).is_ok()
}

/// Why a binary cannot be opened for writing, if it cannot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WriteBlock {
    /// `ETXTBSY`: the binary is currently running. No privilege level can
    /// rewrite a live executable in place - it has to be quit first.
    Busy,
    /// `EACCES`: permissions (typically a root-owned install).
    Denied,
    /// Anything else, with the OS message.
    Other(String),
}

pub fn write_block(path: &Path) -> Option<WriteBlock> {
    match OpenOptions::new().write(true).open(path) {
        Ok(_) => None,
        Err(e) => Some(match e.raw_os_error() {
            Some(26) => WriteBlock::Busy,   // ETXTBSY
            Some(13) => WriteBlock::Denied, // EACCES
            _ => WriteBlock::Other(e.to_string()),
        }),
    }
}

/// Whether we could create a sibling file (needed to take a backup).
pub fn can_write_dir(path: &Path) -> bool {
    let dir = match path.parent() {
        Some(d) => d,
        None => return false,
    };
    let probe = dir.join(format!(".cgp-write-test-{}", std::process::id()));
    match File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

pub fn is_root() -> bool {
    // std has no geteuid; /proc/self/status is always available on Linux.
    if let Ok(s) = std::fs::read_to_string("/proc/self/status") {
        for line in s.lines() {
            if let Some(rest) = line.strip_prefix("Uid:") {
                let uid: Vec<&str> = rest.split_whitespace().collect();
                return uid.first().map(|u| *u == "0").unwrap_or(false);
            }
        }
    }
    false
}
