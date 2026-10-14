//! Finding Chromium-family browsers and Electron/CEF clients.
//!
//! A static list of just a bunch of install paths, plus a shallow walk of the usual
//! install roots filtered to large ELF binaries. A candidate counts only once the
//! signature is actually found inside it.
//!
//! PR your paths here if you want them to be known by this dinky patcher

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Minimum size for a walk candidate; Chromium's main binary is always pretty huge
const MIN_WALK_SIZE: u64 = 16 * 1024 * 1024;

pub const BAKED_PATHS: &[&str] = &[
    // Chromium itself
    "/opt/google/chrome/chrome",
    "/opt/google/chrome-beta/chrome",
    "/opt/google/chrome-unstable/chrome",
    "/usr/bin/chromium",
    "/usr/bin/chromium-browser",
    "/usr/lib/chromium/chromium",
    "/usr/lib/chromium-browser/chromium-browser",
    "/usr/lib64/chromium/chromium",
    "/opt/chromium/chromium",
    "/usr/lib/ungoogled-chromium/ungoogled-chromium",
    "/usr/bin/ungoogled-chromium",
    // Derivatives (ai generated dont @ me i am not installing all of that shitware)
    "/opt/vivaldi/vivaldi-bin",
    "/opt/vivaldi-snapshot/vivaldi-snapshot",
    "/usr/bin/vivaldi-stable",
    "/usr/bin/vivaldi-snapshot",
    "/opt/brave-bin/brave",
    "/opt/brave.com/brave/brave",
    "/usr/bin/brave",
    "/usr/bin/brave-browser",
    "/usr/lib/brave-bin/brave",
    "/usr/lib/brave/brave",
    "/usr/lib/opera/opera",
    "/usr/bin/opera",
    "/opt/microsoft/msedge/msedge",
    "/usr/lib/microsoft-edge/microsoft-edge",
    "/usr/bin/microsoft-edge-stable",
    "/usr/lib/thorium-browser/thorium",
    "/opt/thorium/thorium",
    "/usr/bin/thorium-browser",
    "/opt/naver-whale/naver-whale",
    "/usr/lib/electron/electron",
    "/usr/lib64/electron/electron",
    // CEF hosts (Plex desktop, Spotify-ish wrappers, custom CEF apps)
    "/usr/lib/cef/libcef.so",
    "/opt/cef/libcef.so",
    // Editor / Electron clients (same as above, ai)
    "/usr/share/code/code",
    "/usr/share/code-insiders/code-insiders",
    "/usr/share/code-oss/code-oss",
    "/opt/visual-studio-code/code",
    "/usr/lib/signal-desktop/signal-desktop",
    "/usr/bin/signal-desktop",
    "/usr/lib/slack/slack",
    "/usr/lib/discord/Discord",
    "/usr/share/discord/Discord",
    "/usr/lib/obsidian/obsidian",
    "/usr/bin/obsidian",
    "/usr/lib/notion-app/notion-app",
    "/usr/lib/spotify/spotify",
    "/opt/spotify/spotify",
];

/// Directories walked one or two levels deep for big ELF binaries.
const WALK_ROOTS: &[&str] = &[
    "/opt/*",
    "/opt/*/*",
    "/usr/lib/*",
    "/usr/lib/*/*",
    "/usr/lib64/*",
    "/usr/share/*",
    "/usr/local/lib/*",
    "/var/lib/flatpak/app/*/current/active/files/*",
    "~/.config/*/app-*",
    "~/.config/*/app-*/*",
    "~/.local/share/*",
    "~/.local/lib/*",
    "~/.local/opt/*",
];

/// Directory / file names never worth reading
const NOISE: &[&str] = &[
    "cuda", "dri", "firmware", "modules", "debug", "cmake", "pkgconfig", "include", "jvm", "java",
    "perl5", "ruby", "rustlib", "gcc", "llvm", "clang", "texlive", "node", "node_modules",
    "python3", "go", "docker", "containers", "git", "git-core", "qt5", "qt6", "systemd", "udev",
    "X11", "fonts", "icons", "locale", "man", "doc", "share", "hunspell", "myspell", "firefox",
    "gstreamer-1.0", "pipewire-0.3", "spa-0.2", "sdk", "android",
];

const NOISE_SUFFIXES: &[&str] = &[
    // my crap sometimes
    ".gamma.bak", ".skia-backup", "-crashpad", "chrome-sandbox", "chrome-shadow",
];

pub fn expand_tilde(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return Path::new(&home).join(rest);
        }
    }
    PathBuf::from(p)
}

fn is_noise(name: &str) -> bool {
    if NOISE.contains(&name) {
        return true;
    }
    NOISE_SUFFIXES.iter().any(|s| name.contains(s))
        || name.starts_with('.')
        || name.starts_with("libwebkit")
        || name.starts_with("libmoz")
}

fn looks_like_target_name(name: &str) -> bool {
    // Cheap pre-filter for the walk: keep the names Chromium-family binaries
    // actually use, plus any extension-less executable (Electron app mains).
    if name.ends_with(".so") || name.ends_with(".so.1") {
        return name.contains("cef") || name.contains("chrom") || name.contains("WebEngine");
    }
    if name.contains("chrome-sandbox") || name.contains("-crashpad") {
        return false;
    }
    // Skip obvious data/library suffixes.
    !(name.ends_with(".py")
        || name.ends_with(".dll")
        || name.ends_with(".json")
        || name.ends_with(".pak")
        || name.ends_with(".bin")
        || name.ends_with(".dat"))
}

/// Expand a simple `*`-glob pattern (single-level per component).
fn expand_glob(pattern: &str) -> Vec<PathBuf> {
    let base = expand_tilde(pattern);
    let mut parts: Vec<String> = base
        .to_string_lossy()
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    let mut current: Vec<PathBuf> = vec![PathBuf::from("/")];
    for part in parts.drain(..) {
        let mut next: Vec<PathBuf> = Vec::new();
        if part.contains('*') || part.contains('?') {
            for dir in &current {
                if let Ok(rd) = std::fs::read_dir(dir) {
                    for e in rd.flatten() {
                        let n = e.file_name().to_string_lossy().to_string();
                        if is_noise(&n) {
                            continue;
                        }
                        if matches_simple(&n, &part) {
                            next.push(e.path());
                        }
                    }
                }
            }
        } else {
            for dir in &current {
                let p = dir.join(&part);
                if p.exists() {
                    next.push(p);
                }
            }
        }
        current = next;
        if current.is_empty() {
            break;
        }
    }
    current
}

fn matches_simple(name: &str, pat: &str) -> bool {
    // Support only leading/trailing `*` - enough for `chrome*`, `app-*`, `*bar`.
    if let Some(prefix) = pat.strip_suffix('*') {
        if let Some(mid) = prefix.strip_prefix('*') {
            return name.contains(mid);
        }
        return name.starts_with(prefix);
    }
    if let Some(suffix) = pat.strip_prefix('*') {
        return name.ends_with(suffix);
    }
    name == pat
}

/// Candidate files to scan: baked paths that exist, plus walk hits.
pub fn candidates() -> Vec<PathBuf> {
    let mut set: BTreeSet<PathBuf> = BTreeSet::new();

    for p in BAKED_PATHS {
        let pb = expand_tilde(p);
        if is_regular_file(&pb) {
            set.insert(canonical(&pb));
        }
    }

    for pat in WALK_ROOTS {
        for dir in expand_glob(pat) {
            collect_in_dir(&dir, &mut set);
        }
    }

    set.into_iter().collect()
}

fn is_regular_file(p: &Path) -> bool {
    std::fs::symlink_metadata(p)
        .map(|m| m.is_file())
        .unwrap_or(false)
}

fn canonical(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

fn collect_in_dir(dir: &Path, out: &mut BTreeSet<PathBuf>) {
    let rd = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return,
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if is_noise(&name) || !looks_like_target_name(&name) {
            continue;
        }
        let md = match std::fs::metadata(e.path()) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !md.is_file() || md.len() < MIN_WALK_SIZE {
            continue;
        }
        if !is_elf_magic(&e.path()) {
            continue;
        }
        out.insert(canonical(&e.path()));
    }
}

fn is_elf_magic(p: &Path) -> bool {
    use std::io::Read;
    let mut buf = [0u8; 4];
    match std::fs::File::open(p) {
        Ok(mut f) => f.read_exact(&mut buf).is_ok() && &buf == b"\x7fELF",
        Err(_) => false,
    }
}

/// Human-friendly label for a path (browser/app name)
/// Again ai, looks right, may be wrong
/// refer to note above about shitware
pub fn label_for(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let dir = path
        .parent()
        .and_then(|d| d.file_name())
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let known: &[(&str, &str)] = &[
        ("vivaldi-bin", "Vivaldi"),
        ("vivaldi-snapshot", "Vivaldi Snapshot"),
        ("vivaldi-stable", "Vivaldi"),
        ("chrome", "Google Chrome"),
        ("chromium", "Chromium"),
        ("ungoogled-chromium", "ungoogled-chromium"),
        ("brave", "Brave"),
        ("brave-browser", "Brave"),
        ("opera", "Opera"),
        ("msedge", "Microsoft Edge"),
        ("microsoft-edge", "Microsoft Edge"),
        ("thorium", "Thorium"),
        ("naver-whale", "Naver Whale"),
        ("electron", "Electron (runtime)"),
        ("libcef.so", "libcef (CEF host)"),
        ("code", "VS Code"),
        ("code-insiders", "VS Code Insiders"),
        ("code-oss", "Code - OSS"),
        ("signal-desktop", "Signal"),
        ("slack", "Slack"),
        ("Discord", "Discord"),
        ("obsidian", "Obsidian"),
        ("notion-app", "Notion"),
        ("spotify", "Spotify"),
    ];
    for (k, v) in known {
        if name == *k {
            return v.to_string();
        }
    }
    // Versioned Electron dirs: <app>/app-1.2.3/<Binary>
    if name.to_lowercase() == dir.to_lowercase() || dir.starts_with("app-") {
        if let Some(grandparent) = path
            .parent()
            .and_then(|d| d.parent())
            .and_then(|d| d.file_name())
            .map(|s| s.to_string_lossy().to_string())
        {
            // Don't report "opt (Electron)" for /opt/<App>/<app>: a generic
            // install root carries no information, the app directory does.
            const GENERIC_ROOTS: &[&str] =
                &["opt", "usr", "share", "local", "lib", "lib64", "bin", "home", "sbin"];
            if !GENERIC_ROOTS.contains(&grandparent.as_str()) {
                return format!("{grandparent} (Electron)");
            }
            return dir;
        }
    }
    format!("{name} ({dir})")
}

/// Paths of running processes, mapped from a binary path to PIDs. Used to warn
/// that a patch needs a restart.
pub fn running_pids_for(path: &Path) -> Vec<u32> {
    let target = canonical(path);
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir("/proc") {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if !n.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let exe = e.path().join("exe");
            if let Ok(link) = std::fs::read_link(&exe) {
                if link == target {
                    if let Ok(pid) = n.parse::<u32>() {
                        out.push(pid);
                    }
                }
            }
        }
    }
    out.sort_unstable();
    out
}
