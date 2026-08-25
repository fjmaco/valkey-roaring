//! Shared helpers for unit tests.

/// Deterministic xorshift64 PRNG — keeps randomized tests reproducible.
pub(crate) fn xorshift(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}
