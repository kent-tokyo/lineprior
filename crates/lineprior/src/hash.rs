//! FNV-1a: fast, and -- unlike `std::collections::hash_map::DefaultHasher`,
//! whose docs disclaim the algorithm across Rust releases -- has a fixed
//! public specification that never changes. Both callers in this crate need
//! a hash that stays reproducible for their own purposes (a deterministic
//! eval train/test split; a `BuildConfig` fingerprint), which a hash stdlib
//! doesn't promise to keep stable isn't safe to build on.

pub(crate) fn fnv1a(bytes: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;
    bytes.iter().fold(OFFSET_BASIS, |hash, &byte| {
        (hash ^ byte as u64).wrapping_mul(PRIME)
    })
}
