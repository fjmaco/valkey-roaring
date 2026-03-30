//! valkey-roaring: Error constants matching redis-roaring error messages.

pub const ERR_KEY_NOT_FOUND: &str = "Roaring: key does not exist";
pub const ERR_KEY_EXISTS: &str = "Roaring: key already exist";
pub const ERR_SET_VALUE: &str = "Roaring: error setting value";
pub const ERR_RANGE_TOO_LARGE: &str = "Roaring: range too large: maximum 100000000 elements";
pub const ERR_INVALID_END: &str = "ERR invalid end: must be >= start";
pub const ERR_SYNTAX: &str = "ERR syntax error";
pub const ERR_BAD_BINARY: &str = "ERR bad binary data for roaring";

pub const MAX_RANGE_SIZE: u64 = 100_000_000;
