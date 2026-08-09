//! A hash whose value must stay the same across builds, forever.
//!
//! Two files are named after a config path: the single-instance lock, and the
//! notification repeat-stamp. Both are cross-process rendezvous points — two
//! beckon processes only meet if they independently compute the same name.
//!
//! `DefaultHasher` cannot do that job. `std` documents its algorithm and seeds
//! as unspecified and free to change between Rust releases, so a serve built
//! with one toolchain and a watchdog probe built with another could pick
//! *different* lock files for the same config. The probe would then acquire
//! its lock happily, and two daemons would run at once, both calling
//! `RegisterHotKey` — the second losing every key. That is exactly the failure
//! this project already recorded once, and it would arrive with no error
//! anywhere: the lock is the only thing asserting there is one daemon.
//!
//! FNV-1a is fixed by its specification and has no seed, so the name depends
//! on the path and nothing else.
#![cfg_attr(
    all(not(test), not(any(target_os = "macos", target_os = "windows"))),
    allow(dead_code)
)]

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Name a temp file after `path`, stably.
pub fn for_path(path: &std::path::Path) -> String {
    format!("{:016x}", fnv1a64(path.as_os_str().as_encoded_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The published FNV-1a 64-bit test vectors. If these ever change, every
    /// running beckon stops finding the lock its predecessor made.
    #[test]
    fn matches_the_published_fnv1a_vectors() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn distinct_paths_get_distinct_names() {
        let a = for_path(std::path::Path::new("/tmp/a.toml"));
        let b = for_path(std::path::Path::new("/tmp/b.toml"));
        assert_ne!(a, b);
        assert_eq!(a.len(), 16, "fixed width keeps the filename predictable");
    }

    #[test]
    fn same_path_is_stable_within_a_run() {
        let p = std::path::Path::new("/tmp/apps.toml");
        assert_eq!(for_path(p), for_path(p));
    }
}
