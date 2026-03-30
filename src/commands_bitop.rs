//! valkey-roaring: BITOP command dispatch and all 8 sub-operations.
//!
//! Operations:
//!   AND     — intersection of all sources
//!   OR      — union of all sources
//!   XOR     — symmetric difference of all sources
//!   NOT     — complement of single source (flip bits in [0, max])
//!   ANDOR   — (src[1] | src[2] | ...) & src[0]
//!   DIFF    — src[0] - src[1] - src[2] - ...  (ANDNOT)
//!   DIFF1   — (src[1] | src[2] | ...) - src[0] (ORNOT)
//!   ONE     — bits present in exactly one source

use crate::bitmap_type::RoaringType;
use crate::error::*;
use valkey_module::native_types::ValkeyType;
use valkey_module::{Context, ValkeyError, ValkeyResult, ValkeyString, ValkeyValue};

/// R.BITOP / R64.BITOP — dispatch to sub-operations.
/// Syntax: R.BITOP <op> <destkey> <srckey> [srckey ...]
pub fn handle_bitop<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if args.len() < 4 {
        return Err(ValkeyError::WrongArity);
    }

    let op = args[1].to_string_lossy().to_uppercase();
    let dest_arg = &args[2];
    let src_args = &args[3..];

    // NOT only takes one source key
    if op == "NOT" {
        if src_args.len() != 1 {
            return Err(ValkeyError::WrongArity);
        }
    }

    // Read all source bitmaps (clone to handle dest-is-source aliasing)
    let sources: Vec<T> = src_args
        .iter()
        .map(|arg| {
            let key = ctx.open_key(arg);
            match key.get_value::<T>(vtype) {
                Ok(Some(bm)) => Ok(bm.clone()),
                Ok(None) => Ok(T::new()),
                Err(e) => Err(e),
            }
        })
        .collect::<Result<_, _>>()?;

    let result = match op.as_str() {
        "AND" => op_and(sources),
        "OR" => op_or(sources),
        "XOR" => op_xor(sources),
        "NOT" => op_not(sources),
        "ANDOR" => op_andor(sources),
        "DIFF" => op_andnot(sources),
        "DIFF1" => op_ornot(sources),
        "ONE" => op_one(sources),
        _ => return Err(ValkeyError::Str(ERR_SYNTAX)),
    };

    let cardinality = result.len() as i64;
    let dest = ctx.open_key_writable(dest_arg);
    dest.set_value(vtype, result)?;

    Ok(ValkeyValue::Integer(cardinality))
}

/// AND: intersection of all sources.
fn op_and<T: RoaringType>(sources: Vec<T>) -> T {
    if sources.is_empty() {
        return T::new();
    }
    let mut result = sources[0].clone();
    for src in &sources[1..] {
        result.bitand_assign(src);
    }
    result
}

/// OR: union of all sources.
fn op_or<T: RoaringType>(sources: Vec<T>) -> T {
    if sources.is_empty() {
        return T::new();
    }
    let mut result = sources[0].clone();
    for src in &sources[1..] {
        result.bitor_assign(src);
    }
    result
}

/// XOR: symmetric difference of all sources.
fn op_xor<T: RoaringType>(sources: Vec<T>) -> T {
    if sources.is_empty() {
        return T::new();
    }
    let mut result = sources[0].clone();
    for src in &sources[1..] {
        result.bitxor_assign(src);
    }
    result
}

/// NOT: complement of single source — flip bits in [0, max].
fn op_not<T: RoaringType>(sources: Vec<T>) -> T {
    if sources.is_empty() {
        return T::new();
    }
    let src = &sources[0];
    match src.max_val() {
        Some(max) => {
            // Flip [0, max+1) to include max in the universe
            // We need max + 1 but must handle overflow
            let max_i64 = T::value_to_i64(max);
            match T::Value::try_from(max_i64 + 1) {
                Ok(end) => src.flip_to(end),
                Err(_) => {
                    // max is the maximum value for the type — flip_to would overflow.
                    // Flip [0, max) then toggle max bit.
                    let mut result = src.flip_to(max);
                    result.remove(max);
                    result
                }
            }
        }
        None => T::new(), // empty bitmap → NOT is empty
    }
}

/// ANDOR: (src[1] | src[2] | ...) & src[0]
fn op_andor<T: RoaringType>(sources: Vec<T>) -> T {
    if sources.len() < 2 {
        return T::new();
    }
    // Union of src[1..]
    let mut union = sources[1].clone();
    for src in &sources[2..] {
        union.bitor_assign(src);
    }
    // Intersect with src[0]
    union.bitand_assign(&sources[0]);
    union
}

/// ANDNOT / DIFF: src[0] - src[1] - src[2] - ...
fn op_andnot<T: RoaringType>(sources: Vec<T>) -> T {
    if sources.is_empty() {
        return T::new();
    }
    let mut result = sources[0].clone();
    for src in &sources[1..] {
        result.sub_assign(src);
    }
    result
}

/// ORNOT / DIFF1: (src[1] | src[2] | ...) - src[0]
fn op_ornot<T: RoaringType>(sources: Vec<T>) -> T {
    if sources.len() < 2 {
        return T::new();
    }
    let mut union = sources[1].clone();
    for src in &sources[2..] {
        union.bitor_assign(src);
    }
    union.sub_assign(&sources[0]);
    union
}

/// ONE: bits present in exactly one source.
/// Algorithm: XOR accumulator + intersection tracker to remove duplicates.
fn op_one<T: RoaringType>(sources: Vec<T>) -> T {
    if sources.is_empty() {
        return T::new();
    }
    if sources.len() == 1 {
        return sources[0].clone();
    }
    // `result` tracks XOR accumulator (bits toggled odd number of times)
    // `seen_twice` tracks bits that appeared in 2+ sources
    let mut result = sources[0].clone();
    let mut seen_twice = T::new();

    for src in &sources[1..] {
        // Bits in both result and src were already in some source + this source → duplicates
        let mut overlap = result.clone();
        overlap.bitand_assign(src);
        seen_twice.bitor_assign(&overlap);

        result.bitxor_assign(src);
    }

    // Remove all bits that appeared in 2+ sources
    result.sub_assign(&seen_twice);
    result
}
