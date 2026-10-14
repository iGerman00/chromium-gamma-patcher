//! Engine tests: signature scan, in-place patch, hash-verified restore.
//!
//! These build a synthetic ELF64 with the real immediate pattern planted in
//! an executable PT_LOAD segment, so the whole path is exercised for real
//! (parse program headers -> locate pairs -> write 4 bytes -> verify) without
//! depending on any browser being installed.
//!
//! The layout regression test (`gamma_goes_in_the_gamma_slot`) pins the mistake
//! that once gave me trouble: `patch_in_place()` takes `(contrast, gamma)` in
//! that order, because the 0.2f immediate is the contrast and the 1.2f
//! one the gamma.

use chromium_gamma_patcher::elf::{
    self, CONTRAST_ORIG, GAMMA_ORIG, Pair, SIG_CONTRAST, SIG_GAMMA,
};

const HEADER_LEN: usize = 0x1000;
const CODE_LEN: usize = 0x40;

fn write_u16(v: &mut [u8], off: usize, val: u16) {
    v[off..off + 2].copy_from_slice(&val.to_le_bytes());
}
fn write_u32(v: &mut [u8], off: usize, val: u32) {
    v[off..off + 4].copy_from_slice(&val.to_le_bytes());
}
fn write_u64(v: &mut [u8], off: usize, val: u64) {
    v[off..off + 8].copy_from_slice(&val.to_le_bytes());
}

/// ELF64, little-endian, two PT_LOADs: a read-only header segment and the code
/// segment at 0x1000 which carries the (0.2f, 1.2f) pair. `exec` controls
/// whether the code segment is marked executable, so the segment filter can be
/// tested. `gap` is the distance between the two immediates.
fn synthetic_elf(gap: usize, exec: bool) -> Vec<u8> {
    let mut v = vec![0u8; HEADER_LEN];
    v[0..4].copy_from_slice(b"\x7fELF");
    v[4] = 2; // ELFCLASS64
    v[5] = 1; // ELFDATA2LSB
    write_u64(&mut v, 0x20, 64); // e_phoff
    write_u16(&mut v, 0x36, 56); // e_phentsize
    write_u16(&mut v, 0x38, 2); // e_phnum

    // phdr 0: PT_LOAD, R, covers the headers
    write_u32(&mut v, 64, 1);
    write_u32(&mut v, 64 + 4, 4);
    write_u64(&mut v, 64 + 8, 0);
    write_u64(&mut v, 64 + 0x20, HEADER_LEN as u64);

    // phdr 1: PT_LOAD at 0x1000, optionally executable
    let p1 = 64 + 56;
    write_u32(&mut v, p1, 1);
    write_u32(&mut v, p1 + 4, if exec { 5 } else { 4 });
    write_u64(&mut v, p1 + 8, HEADER_LEN as u64);
    write_u64(&mut v, p1 + 0x20, CODE_LEN as u64);

    let mut code = vec![0u8; CODE_LEN];
    code[0..4].copy_from_slice(&SIG_CONTRAST);
    code[gap..gap + 4].copy_from_slice(&SIG_GAMMA);
    v.extend_from_slice(&code);
    v
}

fn fixture(name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("cgp-test-{name}"));
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn exec_ranges_are_found() {
    let data = synthetic_elf(12, true);
    let ranges = elf::exec_ranges(&data).expect("elf parses");
    assert_eq!(ranges, vec![(HEADER_LEN as u64, (HEADER_LEN + CODE_LEN) as u64)]);
}

#[test]
fn signature_is_located_at_the_right_offsets() {
    let data = synthetic_elf(12, true);
    let pairs = elf::find_pairs(&data);
    assert_eq!(pairs.len(), 1, "exactly one pair expected");
    let p = pairs[0];
    assert_eq!(p.contrast_off, HEADER_LEN as u64, "0.2f immediate offset");
    assert_eq!(p.gamma_off, (HEADER_LEN + 12) as u64, "1.2f immediate offset");
    assert_eq!(elf::read_f32_at(&data, p.contrast_off), Some(CONTRAST_ORIG));
    assert_eq!(elf::read_f32_at(&data, p.gamma_off), Some(GAMMA_ORIG));
}

#[test]
fn gap_must_be_within_8_to_16_bytes() {
    for gap in [8, 9, 16] {
        assert_eq!(elf::find_pairs(&synthetic_elf(gap, true)).len(), 1, "gap {gap}");
    }
    // The immediates are supposed to sit inside one instruction block; a larger
    // distance is some other code, not the getter.
    for gap in [4, 7, 17, 32] {
        assert!(elf::find_pairs(&synthetic_elf(gap, true)).is_empty(), "gap {gap}");
    }
}

#[test]
fn pairs_outside_a_code_segment_are_ignored() {
    // Same bytes, but the segment holding them is not executable: not a match.
    assert!(elf::find_pairs(&synthetic_elf(12, false)).is_empty());
}

#[test]
fn gamma_goes_in_the_gamma_slot() {
    // Regression: patch_in_place(contrast, gamma). Passing them the other way
    // round silently writes 2.2 into the contrast immediate.
    let data = synthetic_elf(12, true);
    let path = fixture("patch-slot", &data);
    let pairs = elf::find_pairs(&data);

    elf::patch_in_place(&path, &pairs, 0.0, 2.2).unwrap();

    let after = std::fs::read(&path).unwrap();
    assert_eq!(elf::read_f32_at(&after, pairs[0].contrast_off), Some(0.0));
    assert_eq!(elf::read_f32_at(&after, pairs[0].gamma_off), Some(2.2));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn patching_removes_the_signature_and_restore_brings_it_back() {
    let data = synthetic_elf(12, true);
    let orig_hash = elf::sha256_hex(&data);
    let path = fixture("patch-restore", &data);
    let pairs = elf::find_pairs(&data);
    assert_eq!(pairs.len(), 1);

    elf::patch_in_place(&path, &pairs, 0.0, 2.2).unwrap();

    // A patched binary is recognised by the *absence* of the signature - this is
    // what the scanner keys its "already patched" state off.
    let patched = std::fs::read(&path).unwrap();
    assert!(elf::find_pairs(&patched).is_empty(), "signature must be gone");
    assert_ne!(elf::sha256_hex(&patched), orig_hash, "hash must change");

    // restore_in_place writes the canonical values back.
    elf::restore_in_place(&path, &pairs).unwrap();
    let restored = std::fs::read(&path).unwrap();
    assert_eq!(elf::sha256_hex(&restored), orig_hash, "byte-identical restore");
    assert_eq!(elf::find_pairs(&restored).len(), 1, "signature is back");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn restore_verified_refuses_a_hash_mismatch() {
    let data = synthetic_elf(12, true);
    let orig_hash = elf::sha256_hex(&data);
    let path = fixture("restore-verify", &data);
    let pairs = elf::find_pairs(&data);
    elf::patch_in_place(&path, &pairs, 0.0, 2.2).unwrap();

    // Wrong recorded hash -> refuse, and leave the file alone.
    let before = std::fs::read(&path).unwrap();
    let err = elf::restore_verified(&path, &pairs, &"0".repeat(64)).unwrap_err();
    assert!(err.contains("refusing to restore"), "got: {err}");
    assert_eq!(std::fs::read(&path).unwrap(), before, "file must be untouched");

    // Right hash -> restores and verifies.
    elf::restore_verified(&path, &pairs, &orig_hash).unwrap();
    assert_eq!(elf::sha256_hex(&std::fs::read(&path).unwrap()), orig_hash);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn backup_path_convention() {
    let p = std::path::Path::new("/opt/vivaldi/vivaldi-bin");
    let bak = elf::backup_path_for(p, "d305899aecc2deadbeef");
    assert_eq!(
        bak,
        std::path::PathBuf::from("/opt/vivaldi/vivaldi-bin.gamma.bak-d305899aecc2"),
        "proper naming of backup file"
    );
}

#[test]
fn pair_codec_round_trips() {
    let p = Pair { contrast_off: 62479446, gamma_off: 62479457 };
    assert_eq!(p.encode(), "62479446:62479457");
    assert_eq!(Pair::decode(&p.encode()), Some(p));
    assert_eq!(Pair::decode("nonsense"), None);
    assert_eq!(Pair::decode("1:"), None);
}
