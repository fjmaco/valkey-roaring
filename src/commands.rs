//! valkey-roaring: Generic command handlers parameterized by RoaringType.

use crate::bitmap_type::RoaringType;
use crate::error::*;
use crate::parse::*;
use std::io::Cursor;
use valkey_module::native_types::ValkeyType;
use valkey_module::{Context, ValkeyError, ValkeyResult, ValkeyString, ValkeyValue};

// ============================================================
// Helper: get or create bitmap from a writable key
// ============================================================
fn get_or_create<'a, T: RoaringType>(
    key: &'a valkey_module::key::ValkeyKeyWritable,
    vtype: &ValkeyType,
) -> Result<&'a mut T, ValkeyError> {
    if key.get_value::<T>(vtype)?.is_none() {
        key.set_value(vtype, T::new())?;
    }
    Ok(key.get_value::<T>(vtype)?.unwrap())
}

fn require_existing<'a, T: RoaringType>(
    key: &'a valkey_module::key::ValkeyKey,
    vtype: &ValkeyType,
) -> Result<&'a T, ValkeyError> {
    key.get_value::<T>(vtype)?
        .ok_or(ValkeyError::Str(ERR_KEY_NOT_FOUND))
}

// ============================================================
// Value parsing helpers — pick u32 or u64 based on T::Value
// ============================================================
pub(crate) fn parse_value<T: RoaringType>(
    arg: &ValkeyString,
    name: &str,
) -> Result<T::Value, ValkeyError> {
    let s = arg.to_string_lossy();
    let val: u64 = s.parse().map_err(|_| {
        ValkeyError::String(format!(
            "ERR invalid {}: must be a non-negative integer",
            name
        ))
    })?;
    T::Value::try_from(val)
        .map_err(|_| ValkeyError::String(format!("ERR invalid {}: value out of range", name)))
}

/// Reply with a bitmap value. Values that fit i64 are integer replies; larger
/// u64 values are decimal bulk strings, matching the C module's ReplyWithUint64.
pub(crate) fn value_reply<T: RoaringType>(v: T::Value) -> ValkeyValue {
    let i = T::value_to_i64(v); // saturates at i64::MAX
    if i == i64::MAX && v.to_string() != i.to_string() {
        ValkeyValue::BulkString(v.to_string())
    } else {
        ValkeyValue::Integer(i)
    }
}

// ============================================================
// R.SETBIT / R64.SETBIT
// ============================================================
pub fn handle_setbit<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() != 4 {
        return Err(ValkeyError::WrongArity);
    }
    let offset = parse_value::<T>(&args[2], "offset")?;
    let value = parse_bool(&args[3], "value")?;

    let key = ctx.open_key_writable(&args[1]);
    let bitmap = get_or_create::<T>(&key, vtype)?;
    let previous = bitmap.contains(offset);

    if value {
        bitmap.insert(offset);
    } else {
        bitmap.remove(offset);
    }
    ctx.replicate_verbatim();

    Ok(ValkeyValue::Integer(previous as i64))
}

// ============================================================
// R.GETBIT / R64.GETBIT
// ============================================================
pub fn handle_getbit<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() != 3 {
        return Err(ValkeyError::WrongArity);
    }
    let offset = parse_value::<T>(&args[2], "offset")?;

    let key = ctx.open_key(&args[1]);
    match key.get_value::<T>(vtype)? {
        Some(bitmap) => Ok(ValkeyValue::Integer(bitmap.contains(offset) as i64)),
        None => Ok(ValkeyValue::Integer(0)),
    }
}

// ============================================================
// R.GETBITS / R64.GETBITS
// ============================================================
pub fn handle_getbits<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() < 3 {
        return Err(ValkeyError::WrongArity);
    }
    let offsets: Vec<T::Value> = args[2..]
        .iter()
        .map(|a| parse_value::<T>(a, "offset"))
        .collect::<Result<_, _>>()?;

    let key = ctx.open_key(&args[1]);
    // A missing key replies an empty array (redis-roaring semantics), not
    // a zero per offset.
    let results = match key.get_value::<T>(vtype)? {
        Some(bitmap) => bitmap.contains_many(&offsets),
        None => Vec::new(),
    };

    Ok(ValkeyValue::Array(
        results
            .into_iter()
            .map(|b| ValkeyValue::Integer(b as i64))
            .collect(),
    ))
}

// ============================================================
// R.CLEARBITS / R64.CLEARBITS
// ============================================================
pub fn handle_clearbits<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() < 3 {
        return Err(ValkeyError::WrongArity);
    }
    // A trailing literal COUNT switches the reply from OK to the number of
    // bits actually cleared (redis-roaring semantics).
    let mut offset_args = &args[2..];
    let count_mode = offset_args
        .last()
        .is_some_and(|a| a.to_string_lossy() == "COUNT");
    if count_mode {
        offset_args = &offset_args[..offset_args.len() - 1];
    }
    let offsets: Vec<T::Value> = offset_args
        .iter()
        .map(|a| parse_value::<T>(a, "offset"))
        .collect::<Result<_, _>>()?;

    let key = ctx.open_key_writable(&args[1]);
    match key.get_value::<T>(vtype)? {
        Some(bitmap) => {
            let count = bitmap.remove_many_counted(&offsets);
            ctx.replicate_verbatim();
            if count_mode {
                Ok(ValkeyValue::Integer(count as i64))
            } else {
                Ok(ValkeyValue::SimpleStringStatic("OK"))
            }
        }
        None => Ok(ValkeyValue::Null),
    }
}

// ============================================================
// R.CLEAR / R64.CLEAR
// ============================================================
pub fn handle_clear<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() != 2 {
        return Err(ValkeyError::WrongArity);
    }
    let key = ctx.open_key_writable(&args[1]);
    match key.get_value::<T>(vtype)? {
        Some(bitmap) => {
            let card = bitmap.len();
            bitmap.clear();
            ctx.replicate_verbatim();
            Ok(ValkeyValue::Integer(card as i64))
        }
        None => Ok(ValkeyValue::Null),
    }
}

// ============================================================
// R.SETINTARRAY / R64.SETINTARRAY
// ============================================================
pub fn handle_setintarray<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() < 3 {
        return Err(ValkeyError::WrongArity);
    }
    let vals: Vec<T::Value> = args[2..]
        .iter()
        .map(|a| parse_value::<T>(a, "value"))
        .collect::<Result<_, _>>()?;

    let key = ctx.open_key_writable(&args[1]);
    let bm = T::from_values(&vals);
    key.set_value(vtype, bm)?;
    ctx.replicate_verbatim();

    Ok(ValkeyValue::SimpleStringStatic("OK"))
}

// ============================================================
// R.GETINTARRAY / R64.GETINTARRAY
// ============================================================
pub fn handle_getintarray<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() != 2 {
        return Err(ValkeyError::WrongArity);
    }
    let key = ctx.open_key(&args[1]);
    match key.get_value::<T>(vtype)? {
        Some(bitmap) => {
            let arr: Vec<ValkeyValue> = bitmap.iter_values().map(|v| value_reply::<T>(v)).collect();
            Ok(ValkeyValue::Array(arr))
        }
        None => Ok(ValkeyValue::Array(vec![])),
    }
}

// ============================================================
// R.APPENDINTARRAY / R64.APPENDINTARRAY
// ============================================================
pub fn handle_appendintarray<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() < 3 {
        return Err(ValkeyError::WrongArity);
    }
    let vals: Vec<T::Value> = args[2..]
        .iter()
        .map(|a| parse_value::<T>(a, "value"))
        .collect::<Result<_, _>>()?;

    let key = ctx.open_key_writable(&args[1]);
    let bitmap = get_or_create::<T>(&key, vtype)?;
    bitmap.insert_many(&vals);
    ctx.replicate_verbatim();

    Ok(ValkeyValue::SimpleStringStatic("OK"))
}

// ============================================================
// R.DELETEINTARRAY / R64.DELETEINTARRAY
// ============================================================
pub fn handle_deleteintarray<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() < 3 {
        return Err(ValkeyError::WrongArity);
    }
    let vals: Vec<T::Value> = args[2..]
        .iter()
        .map(|a| parse_value::<T>(a, "value"))
        .collect::<Result<_, _>>()?;

    let key = ctx.open_key_writable(&args[1]);
    let bitmap = get_or_create::<T>(&key, vtype)?;
    bitmap.remove_many(&vals);
    ctx.replicate_verbatim();

    Ok(ValkeyValue::SimpleStringStatic("OK"))
}

// ============================================================
// R.RANGEINTARRAY / R64.RANGEINTARRAY
// ============================================================
pub fn handle_rangeintarray<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() != 4 {
        return Err(ValkeyError::WrongArity);
    }
    let start = parse_value::<T>(&args[2], "start")?;
    let end = parse_value::<T>(&args[3], "end")?;

    // start/end are 0-based POSITIONS in the sorted value array (pagination),
    // matching redis-roaring: elements at indexes [start, end], truncated at
    // the cardinality. An inverted range replies empty.
    let start = T::value_to_i64(start) as u64;
    let end = T::value_to_i64(end) as u64;
    if start > end {
        return Ok(ValkeyValue::Array(vec![]));
    }
    if end - start + 1 > MAX_RANGE_SIZE {
        return Err(ValkeyError::Str(ERR_RANGE_TOO_LARGE));
    }

    let key = ctx.open_key(&args[1]);
    match key.get_value::<T>(vtype)? {
        Some(bitmap) => {
            let mut arr = Vec::new();
            for i in start..=end {
                match bitmap.select(i) {
                    Some(v) => arr.push(value_reply::<T>(v)),
                    None => break,
                }
            }
            Ok(ValkeyValue::Array(arr))
        }
        None => Ok(ValkeyValue::Array(vec![])),
    }
}

// ============================================================
// R.SETBITARRAY / R64.SETBITARRAY
// ============================================================
pub fn handle_setbitarray<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() != 3 {
        return Err(ValkeyError::WrongArity);
    }
    let bits = args[2].to_string_lossy();
    let bm = T::from_bit_array(bits.as_bytes());

    let key = ctx.open_key_writable(&args[1]);
    key.set_value(vtype, bm)?;
    ctx.replicate_verbatim();

    Ok(ValkeyValue::SimpleStringStatic("OK"))
}

// ============================================================
// R.GETBITARRAY / R64.GETBITARRAY
// ============================================================
pub fn handle_getbitarray<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() != 2 {
        return Err(ValkeyError::WrongArity);
    }
    let key = ctx.open_key(&args[1]);
    match key.get_value::<T>(vtype)? {
        Some(bitmap) => {
            // The reply is max+1 bytes; refuse instead of risking an
            // allocation-failure abort on huge maxima.
            if let Some(max) = bitmap.max_val() {
                if T::value_to_i64(max) as u64 >= MAX_RANGE_SIZE {
                    return Err(ValkeyError::Str(ERR_RANGE_TOO_LARGE));
                }
            }
            let bits = bitmap.to_bit_array();
            let s = String::from_utf8(bits).unwrap_or_default();
            Ok(ValkeyValue::BulkString(s))
        }
        None => Ok(ValkeyValue::BulkString(String::new())),
    }
}

// ============================================================
// R.SETRANGE / R64.SETRANGE
// ============================================================
pub fn handle_setrange<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() != 4 {
        return Err(ValkeyError::WrongArity);
    }
    let start = parse_value::<T>(&args[2], "start")?;
    let end = parse_value::<T>(&args[3], "end")?;

    if end < start {
        return Err(ValkeyError::Str(ERR_INVALID_END));
    }

    let key = ctx.open_key_writable(&args[1]);
    let bitmap = get_or_create::<T>(&key, vtype)?;
    // End-exclusive [start, end), matching redis-roaring / CRoaring add_range.
    bitmap.insert_range_exclusive(start, end);
    ctx.replicate_verbatim();

    Ok(ValkeyValue::SimpleStringStatic("OK"))
}

// ============================================================
// R.SETFULL / R64.SETFULL
// ============================================================
pub fn handle_setfull<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() != 2 {
        return Err(ValkeyError::WrongArity);
    }
    let key = ctx.open_key_writable(&args[1]);
    if key.get_value::<T>(vtype)?.is_some() {
        return Err(ValkeyError::Str(ERR_KEY_EXISTS));
    }

    let bm = T::full();
    key.set_value(vtype, bm)?;
    ctx.replicate_verbatim();

    Ok(ValkeyValue::SimpleStringStatic("OK"))
}

// ============================================================
// R.BITCOUNT / R64.BITCOUNT
// ============================================================
pub fn handle_bitcount<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() != 2 {
        return Err(ValkeyError::WrongArity);
    }
    let key = ctx.open_key(&args[1]);
    match key.get_value::<T>(vtype)? {
        Some(bitmap) => Ok(ValkeyValue::Integer(bitmap.len() as i64)),
        None => Ok(ValkeyValue::Integer(0)),
    }
}

// ============================================================
// R.BITPOS / R64.BITPOS
// ============================================================
pub fn handle_bitpos<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() != 3 {
        return Err(ValkeyError::WrongArity);
    }
    let bit = parse_bool(&args[2], "bit")?;

    let key = ctx.open_key(&args[1]);
    match key.get_value::<T>(vtype)? {
        Some(bitmap) => {
            if bit {
                // First set bit
                match bitmap.select(0) {
                    Some(v) => Ok(value_reply::<T>(v)),
                    None => Ok(ValkeyValue::Integer(-1)),
                }
            } else {
                // First unset bit
                match bitmap.nth_absent(1) {
                    Some(v) => Ok(value_reply::<T>(v)),
                    None => Ok(ValkeyValue::Integer(-1)),
                }
            }
        }
        None => {
            if bit {
                Ok(ValkeyValue::Integer(-1))
            } else {
                // Empty bitmap: first absent bit is 0
                Ok(ValkeyValue::Integer(0))
            }
        }
    }
}

// ============================================================
// R.MIN / R64.MIN
// ============================================================
pub fn handle_min<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() != 2 {
        return Err(ValkeyError::WrongArity);
    }
    let key = ctx.open_key(&args[1]);
    match key.get_value::<T>(vtype)? {
        Some(bitmap) => match bitmap.min_val() {
            Some(v) => Ok(value_reply::<T>(v)),
            None => Ok(ValkeyValue::Integer(-1)),
        },
        None => Ok(ValkeyValue::Integer(-1)),
    }
}

// ============================================================
// R.MAX / R64.MAX
// ============================================================
pub fn handle_max<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() != 2 {
        return Err(ValkeyError::WrongArity);
    }
    let key = ctx.open_key(&args[1]);
    match key.get_value::<T>(vtype)? {
        Some(bitmap) => match bitmap.max_val() {
            Some(v) => Ok(value_reply::<T>(v)),
            None => Ok(ValkeyValue::Integer(-1)),
        },
        None => Ok(ValkeyValue::Integer(-1)),
    }
}

// ============================================================
// R.OPTIMIZE / R64.OPTIMIZE
// ============================================================
pub fn handle_optimize<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() < 2 || args.len() > 3 {
        return Err(ValkeyError::WrongArity);
    }
    let key = ctx.open_key_writable(&args[1]);
    match key.get_value::<T>(vtype)? {
        Some(bitmap) => {
            bitmap.optimize();
            ctx.replicate_verbatim();
            Ok(ValkeyValue::SimpleStringStatic("OK"))
        }
        None => Ok(ValkeyValue::SimpleStringStatic("OK")),
    }
}

// ============================================================
// R.CONTAINS / R64.CONTAINS
// ============================================================
pub fn handle_contains<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() < 3 || args.len() > 4 {
        return Err(ValkeyError::WrongArity);
    }

    let key1 = ctx.open_key(&args[1]);
    let b1 = require_existing::<T>(&key1, vtype)?;
    let key2 = ctx.open_key(&args[2]);
    let b2 = require_existing::<T>(&key2, vtype)?;

    let mode = if args.len() == 4 {
        args[3].to_string_lossy().to_uppercase()
    } else {
        "NONE".to_string()
    };

    let result = match mode.as_str() {
        // "NONE" is the implicit default only; upstream rejects it as an
        // explicit token, so an accepted literal NONE would be a divergence.
        "NONE" if args.len() == 3 => !b1.is_disjoint(b2),
        "ALL" => b2.is_subset(b1),
        "ALL_STRICT" => b2.is_subset(b1) && b1 != b2,
        "EQ" => b1 == b2,
        _ => {
            return Err(ValkeyError::String(format!(
                "ERR invalid mode argument: {}",
                mode
            )))
        }
    };

    Ok(ValkeyValue::Integer(result as i64))
}

// ============================================================
// R.JACCARD / R64.JACCARD
// ============================================================
pub fn handle_jaccard<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() != 3 {
        return Err(ValkeyError::WrongArity);
    }

    let key1 = ctx.open_key(&args[1]);
    let b1 = require_existing::<T>(&key1, vtype)?;
    let key2 = ctx.open_key(&args[2]);
    let b2 = require_existing::<T>(&key2, vtype)?;

    let union = b1.union_len(b2);
    if union == 0 {
        return Ok(ValkeyValue::Float(0.0));
    }
    let intersection = b1.intersection_len(b2);
    let jaccard = intersection as f64 / union as f64;

    Ok(ValkeyValue::Float(jaccard))
}

// ============================================================
// R.DIFF / R64.DIFF (separate command, not BITOP DIFF)
// ============================================================
pub fn handle_diff<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() != 4 {
        return Err(ValkeyError::WrongArity);
    }

    // Read sources first
    let key1 = ctx.open_key(&args[2]);
    let b1 = require_existing::<T>(&key1, vtype)?.clone();
    let key2 = ctx.open_key(&args[3]);
    let b2 = require_existing::<T>(&key2, vtype)?.clone();

    let result = b1.sub_owned(b2);

    let dest = ctx.open_key_writable(&args[1]);
    dest.set_value(vtype, result)?;
    ctx.replicate_verbatim();

    Ok(ValkeyValue::SimpleStringStatic("OK"))
}

// ============================================================
// R.EXPORT / R64.EXPORT
// ============================================================
pub fn handle_export<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() != 2 {
        return Err(ValkeyError::WrongArity);
    }
    let key = ctx.open_key_writable(&args[1]);
    let bitmap = match key.get_value::<T>(vtype)? {
        Some(bm) => bm,
        None => return Err(ValkeyError::Str(ERR_KEY_NOT_FOUND)),
    };

    bitmap.optimize();

    let size = bitmap.serialized_size();
    let mut buf = Vec::with_capacity(size);
    bitmap
        .serialize_into(&mut buf)
        .map_err(|_| ValkeyError::Str("ERR serialization failed"))?;

    Ok(ValkeyValue::StringBuffer(buf))
}

// ============================================================
// R.IMPORT / R64.IMPORT
// ============================================================
pub fn handle_import<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() != 3 {
        return Err(ValkeyError::WrongArity);
    }
    let data = args[2].as_slice();
    let new_bitmap =
        T::deserialize_from(Cursor::new(data)).map_err(|_| ValkeyError::Str(ERR_BAD_BINARY))?;

    let key = ctx.open_key_writable(&args[1]);
    match key.get_value::<T>(vtype)? {
        Some(existing) => {
            // OR-merge into existing key
            existing.bitor_assign(&new_bitmap);
        }
        None => {
            key.set_value(vtype, new_bitmap)?;
        }
    }

    ctx.replicate_verbatim();

    // Return cardinality after import
    let bitmap = key.get_value::<T>(vtype)?.unwrap();
    Ok(ValkeyValue::Integer(bitmap.len() as i64))
}

// ============================================================
// R.STAT (shared handler — detects type at runtime)
// This is implemented in lib.rs since it needs both types.
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use roaring::{RoaringBitmap, RoaringTreemap};

    #[test]
    fn value_reply_integer_for_representable_values() {
        assert_eq!(value_reply::<RoaringBitmap>(0), ValkeyValue::Integer(0));
        assert_eq!(value_reply::<RoaringBitmap>(5), ValkeyValue::Integer(5));
        assert_eq!(
            value_reply::<RoaringBitmap>(u32::MAX),
            ValkeyValue::Integer(u32::MAX as i64)
        );
        assert_eq!(
            value_reply::<RoaringTreemap>(i64::MAX as u64),
            ValkeyValue::Integer(i64::MAX)
        );
    }

    #[test]
    fn value_reply_string_above_i64_max() {
        // Matches the C module's ReplyWithUint64: decimal bulk string.
        assert_eq!(
            value_reply::<RoaringTreemap>(i64::MAX as u64 + 1),
            ValkeyValue::BulkString("9223372036854775808".to_string())
        );
        assert_eq!(
            value_reply::<RoaringTreemap>(u64::MAX),
            ValkeyValue::BulkString("18446744073709551615".to_string())
        );
    }
}
