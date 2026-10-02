//! valkey-roaring: Error constants matching redis-roaring error messages.

pub const ERR_KEY_NOT_FOUND: &str = "Roaring: key does not exist";
pub const ERR_KEY_EXISTS: &str = "Roaring: key already exist";
pub const ERR_RANGE_TOO_LARGE: &str = "Roaring: range too large: maximum 100000000 elements";
pub const ERR_SYNTAX: &str = "ERR syntax error";
pub const ERR_BAD_BINARY: &str = "ERR bad binary data for roaring";

/// Most elements a reply may list (RANGEINTARRAY, GETINTARRAY, GETBITARRAY):
/// upstream's BITMAP_MAX_RANGE_SIZE.
pub const MAX_RANGE_SIZE: u64 = 100_000_000;

/// Most values one write may materialize as a contiguous range (SETRANGE,
/// SETFULL, the [0, last] universe of BITOP NOT): 2^38, i.e. 4,194,304 full
/// containers, about 200 MB as run containers. A full 32-bit space (2^32,
/// R.SETFULL) fits comfortably; R64.SETFULL (2^64) and ranges or NOT
/// universes reaching far into the 64-bit space would need billions of
/// containers, which an unguarded write tried to allocate until the server
/// was killed. Valkey-roaring addition; upstream has no such guard.
pub const MAX_STORED_RANGE: u64 = 1 << 38;
pub const ERR_STORED_RANGE_TOO_LARGE: &str =
    "Roaring: range too large: maximum 274877906944 elements";
