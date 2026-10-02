//! valkey-roaring: Error constants. Their text is reply text, kept stable
//! for compatibility. The range refusals, whose text carries the configured
//! limit, are built by limits::range_too_large.

pub const ERR_KEY_NOT_FOUND: &str = "Roaring: key does not exist";
pub const ERR_KEY_EXISTS: &str = "Roaring: key already exist";
pub const ERR_SYNTAX: &str = "ERR syntax error";
pub const ERR_BAD_BINARY: &str = "ERR bad binary data for roaring";
