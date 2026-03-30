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
        .ok_or_else(|| ValkeyError::Str(ERR_KEY_NOT_FOUND))
}

// ============================================================
// Value parsing helpers — pick u32 or u64 based on T::Value
// ============================================================
fn parse_value<T: RoaringType>(arg: &ValkeyString, name: &str) -> Result<T::Value, ValkeyError> {
    // We need to parse into T::Value. Use i64 as intermediate since ValkeyString
    // provides that. We parse the string manually for u32/u64 range.
    let s = arg.to_string_lossy();
    let val: u64 = s.parse().map_err(|_| {
        ValkeyError::String(format!("ERR invalid {}: must be a non-negative integer", name))
    })?;
    // Try to convert u64 -> T::Value via i64 intermediate
    let as_i64 = val as i64;
    T::Value::try_from(as_i64).map_err(|_| {
        ValkeyError::String(format!("ERR invalid {}: value out of range", name))
    })
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
    let results = match key.get_value::<T>(vtype)? {
        Some(bitmap) => bitmap.contains_many(&offsets),
        None => vec![false; offsets.len()],
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
    let offsets: Vec<T::Value> = args[2..]
        .iter()
        .map(|a| parse_value::<T>(a, "offset"))
        .collect::<Result<_, _>>()?;

    let key = ctx.open_key_writable(&args[1]);
    match key.get_value::<T>(vtype)? {
        Some(bitmap) => {
            let count = bitmap.remove_many_counted(&offsets);
            Ok(ValkeyValue::Integer(count as i64))
        }
        None => Ok(ValkeyValue::Integer(0)),
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
            let arr: Vec<ValkeyValue> = bitmap
                .iter_values()
                .map(|v| ValkeyValue::Integer(T::value_to_i64(v)))
                .collect();
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

    let key = ctx.open_key(&args[1]);
    match key.get_value::<T>(vtype)? {
        Some(bitmap) => {
            let arr: Vec<ValkeyValue> = bitmap
                .iter_range(start, end)
                .map(|v| ValkeyValue::Integer(T::value_to_i64(v)))
                .collect();
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
    bitmap.insert_range_inclusive(start, end);

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
                    Some(v) => Ok(ValkeyValue::Integer(T::value_to_i64(v))),
                    None => Ok(ValkeyValue::Integer(-1)),
                }
            } else {
                // First unset bit
                match bitmap.nth_absent(1) {
                    Some(v) => Ok(ValkeyValue::Integer(T::value_to_i64(v))),
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
            Some(v) => Ok(ValkeyValue::Integer(T::value_to_i64(v))),
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
            Some(v) => Ok(ValkeyValue::Integer(T::value_to_i64(v))),
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
        "NONE" => !b1.is_disjoint(b2),
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

    // Return cardinality after import
    let bitmap = key.get_value::<T>(vtype)?.unwrap();
    Ok(ValkeyValue::Integer(bitmap.len() as i64))
}

// ============================================================
// R.STAT (shared handler — detects type at runtime)
// This is implemented in lib.rs since it needs both types.
// ============================================================
