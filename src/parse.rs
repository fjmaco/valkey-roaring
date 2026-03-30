//! valkey-roaring: Argument parsing utilities matching redis-roaring conventions.

use valkey_module::{ValkeyError, ValkeyString};

pub fn parse_u32(arg: &ValkeyString, name: &str) -> Result<u32, ValkeyError> {
    let s = arg.to_string_lossy();
    s.parse::<u32>().map_err(|_| {
        ValkeyError::String(format!(
            "ERR invalid {}: must be an unsigned 32 bit integer",
            name
        ))
    })
}

pub fn parse_u64(arg: &ValkeyString, name: &str) -> Result<u64, ValkeyError> {
    let s = arg.to_string_lossy();
    s.parse::<u64>().map_err(|_| {
        ValkeyError::String(format!(
            "ERR invalid {}: must be an unsigned 64 bit integer",
            name
        ))
    })
}

pub fn parse_bool(arg: &ValkeyString, name: &str) -> Result<bool, ValkeyError> {
    let v = parse_u64(arg, name)?;
    match v {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(ValkeyError::String(format!(
            "ERR invalid {}: must be either 0 or 1",
            name
        ))),
    }
}
