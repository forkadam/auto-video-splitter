use std::collections::HashSet;
use std::fs;
use std::path::Path;

use tempfile::TempDir;
use vsplit_core::hash::{bytes_sha256_hex, content_hash, normalize_hash_hex, partial_hash};
use vsplit_core::keyframes::{parse_keyframes_str, snap_preceding};
use vsplit_core::paths::{normalize_rel_path, resolve_rel_path, to_posix_rel_path, validate_slug};
use vsplit_core::slugify::{resolve_collision, slugify};

#[test]
fn slugify_handles_windows_reserved_names_and_edge_cases() {
    for reserved in [
        "CON.mp4", "prn.mov", "AUX.mkv", "nul.avi", "COM1.mp4", "com9.mp4", "LPT1.mp4", "lpt9.mkv",
    ] {
        let s = slugify(reserved);
        assert!(
            s.starts_with('_'),
            "expected underscore prefix for {reserved}, got {s}"
        );
    }

    assert_eq!(
        slugify("My Trip__2025 (Final)!!.MOV"),
        "my-trip-2025-(final)!!"
    );
    assert_eq!(slugify(r#"Movie<>:"|?*.mkv"#), "movie");
    assert_eq!(slugify("a\x00b.mp4"), "ab");
    assert_eq!(slugify("tab\there.mp4"), "tab-here");
    assert_eq!(slugify("...---   ---...mp4"), "_unnamed");

    let long_name = format!("{}.mp4", "a".repeat(120));
    assert_eq!(slugify(&long_name).len(), 64);
}

#[test]
fn resolve_collision_case_insensitive_live_and_retired() {
    let live: HashSet<String> = ["Clip".into(), "clip-2".into()].into_iter().collect();
    let retired: HashSet<String> = ["CLIP-3".into()].into_iter().collect();
    assert_eq!(
        resolve_collision("clip", &live, &retired).unwrap(),
        "clip-4"
    );

    // Exhaust 2..99 -> error
    let mut full: HashSet<String> = ["x".into()].into_iter().collect();
    for n in 2..100 {
        full.insert(format!("x-{n}"));
    }
    assert!(resolve_collision("x", &full, &HashSet::new()).is_err());
}

#[test]
fn paths_normalize_posix_and_block_traversal() {
    assert_eq!(normalize_rel_path(r"sub\dir\clip.mp4"), "sub/dir/clip.mp4");
    assert_eq!(
        to_posix_rel_path(Path::new("sub").join("dir").join("clip.mp4").as_path()),
        "sub/dir/clip.mp4"
    );
    let resolved = resolve_rel_path(Path::new("/root"), r"a\b\c.mp4");
    assert_eq!(
        resolved,
        Path::new("/root").join("a").join("b").join("c.mp4")
    );

    assert!(validate_slug("valid-slug_1").is_ok());
    for bad in ["", ".", "..", "a/b", r"a\b", "a\0b"] {
        assert!(
            validate_slug(bad).is_err(),
            "expected invalid slug: {bad:?}"
        );
    }
}

#[test]
fn hash_and_keyframes_edge_cases() {
    let dir = TempDir::new().unwrap();
    let small = dir.path().join("small.bin");
    fs::write(&small, b"hello world").unwrap();
    let ph = partial_hash(&small).unwrap();
    let ch = content_hash(&small).unwrap();
    assert!(ph.starts_with("sha256:"));
    assert_eq!(ph.len(), 7 + 64);
    assert_ne!(ph, ch); // partial_hash includes 8-byte big-endian size suffix
    assert_eq!(normalize_hash_hex(&ch), bytes_sha256_hex(b"hello world"));

    // >2MB file exercises head+tail seek path
    let big = dir.path().join("big.bin");
    let mut data = vec![0xAAu8; 2 * 1024 * 1024 + 512];
    data[0] = 0x01;
    let last = data.len() - 1;
    data[last] = 0xFF;
    fs::write(&big, &data).unwrap();
    let ph_big_1 = partial_hash(&big).unwrap();
    // Mutating the middle byte outside first/last 1MB keeps partial_hash identical
    data[1024 * 1024 + 100] = 0x55;
    fs::write(&big, &data).unwrap();
    let ph_big_2 = partial_hash(&big).unwrap();
    assert_eq!(ph_big_1, ph_big_2);

    // Keyframes CSV with Windows SEI extra columns
    let csv = "0.000000,\n1.000000,side_data\n2.500000\nnot-a-number\n\n5.000000\n";
    let kfs = parse_keyframes_str(csv);
    assert_eq!(kfs, vec![0.0, 1.0, 2.5, 5.0]);
    assert_eq!(snap_preceding(2.49, &kfs), 1.0);
    assert_eq!(snap_preceding(2.50, &kfs), 2.5);
    assert_eq!(snap_preceding(0.0, &kfs), 0.0);
    assert_eq!(snap_preceding(1.0, &[]), 0.0);
}
