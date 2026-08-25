//! valkey-roaring: BITOP command dispatch and all 8 sub-operations.
//!
//! Operations:
//!   AND     — intersection of all sources
//!   OR      — union of all sources
//!   XOR     — symmetric difference of all sources
//!   NOT     — complement of single source over [0, max(last, src_max)]
//!             Syntax: R.BITOP NOT <dest> <src> [last]
//!   ANDOR   — (src[1] | src[2] | ...) & src[0]
//!   DIFF    — src[0] - src[1] - src[2] - ...  (ANDNOT)
//!   DIFF1   — (src[1] | src[2] | ...) - src[0] (ORNOT)
//!   ONE     — bits present in exactly one source

use crate::bitmap_type::RoaringType;
use crate::commands::parse_value;
use crate::error::*;
use valkey_module::native_types::ValkeyType;
use valkey_module::{Context, ValkeyError, ValkeyResult, ValkeyString, ValkeyValue};

/// Operations that take a destination plus one or more source keys.
pub fn is_variadic_op(op: &str) -> bool {
    matches!(
        op,
        "AND" | "OR" | "XOR" | "ANDOR" | "DIFF" | "DIFF1" | "ONE"
    )
}

/// Answer a keys-position request (COMMAND GETKEYS, cluster routing, ACL).
///
/// The BITOP key layout is not expressible as a static first/last/step spec:
/// args[1] is the operation and, for NOT, a trailing `last` argument is not a
/// key. NOT reports positions 2..3 only; variadic ops report 2..end. Unknown
/// operations and invalid arities report nothing — execution rejects them.
fn report_bitop_keys(ctx: &Context, args: &[ValkeyString]) {
    if args.len() < 4 {
        return;
    }
    let op = args[1].to_string_lossy().to_uppercase();
    if op == "NOT" {
        if args.len() <= 5 {
            ctx.key_at_pos(2);
            ctx.key_at_pos(3);
        }
    } else if is_variadic_op(&op) && args.len() >= 5 {
        for pos in 2..args.len() {
            ctx.key_at_pos(pos as i32);
        }
    }
}

/// R.BITOP / R64.BITOP — dispatch to sub-operations.
/// Syntax: R.BITOP NOT <destkey> <srckey> [last]
///         R.BITOP <op> <destkey> <srckey> [srckey ...]
pub fn handle_bitop<T: RoaringType>(
    ctx: &Context,
    args: Vec<ValkeyString>,
    vtype: &ValkeyType,
) -> ValkeyResult {
    if ctx.is_keys_position_request() {
        report_bitop_keys(ctx, &args);
        return Ok(ValkeyValue::NoReply);
    }

    if args.len() < 4 {
        return Err(ValkeyError::WrongArity);
    }

    let op = args[1].to_string_lossy().to_uppercase();

    if op == "NOT" {
        return handle_bitop_not::<T>(ctx, &args, vtype);
    }
    if !is_variadic_op(&op) {
        return Err(ValkeyError::Str(ERR_SYNTAX));
    }
    // Variadic operations need at least two sources (redis-roaring arity).
    if args.len() < 5 {
        return Err(ValkeyError::WrongArity);
    }

    // Read all source bitmaps (clone to handle dest-is-source aliasing)
    let sources: Vec<T> = args[3..]
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
        "ANDOR" => op_andor(sources),
        "DIFF" => op_andnot(sources),
        "DIFF1" => op_ornot(sources),
        "ONE" => op_one(sources),
        _ => unreachable!(),
    };

    let cardinality = result.len() as i64;
    let dest = ctx.open_key_writable(&args[2]);
    dest.set_value(vtype, result)?;
    ctx.replicate_verbatim();

    Ok(ValkeyValue::Integer(cardinality))
}

/// NOT: complement of a single source within the universe [0, last].
///
/// Without an explicit `last` the universe ends at the source's max value; an
/// explicit `last` below the source max is raised to it. A missing or empty
/// source with no `last` stores an empty bitmap and replies 0; with `last` it
/// stores the full range [0, last].
fn handle_bitop_not<T: RoaringType>(
    ctx: &Context,
    args: &[ValkeyString],
    vtype: &ValkeyType,
) -> ValkeyResult {
    let last = match args.len() {
        4 => None,
        5 => Some(parse_value::<T>(&args[4], "last")?),
        _ => return Err(ValkeyError::WrongArity),
    };

    let src = {
        let key = ctx.open_key(&args[3]);
        match key.get_value::<T>(vtype)? {
            Some(bm) => bm.clone(),
            None => T::new(),
        }
    };

    let result = match (src.max_val(), last) {
        (None, None) => T::new(),
        (None, Some(last)) => src.flip_inclusive(last),
        (Some(max), None) => src.flip_inclusive(max),
        (Some(max), Some(last)) => src.flip_inclusive(if last > max { last } else { max }),
    };

    let cardinality = result.len() as i64;
    let dest = ctx.open_key_writable(&args[2]);
    dest.set_value(vtype, result)?;
    ctx.replicate_verbatim();

    Ok(ValkeyValue::Integer(cardinality))
}

/// AND: intersection of all sources.
pub fn op_and<T: RoaringType>(sources: Vec<T>) -> T {
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
pub fn op_or<T: RoaringType>(sources: Vec<T>) -> T {
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
pub fn op_xor<T: RoaringType>(sources: Vec<T>) -> T {
    if sources.is_empty() {
        return T::new();
    }
    let mut result = sources[0].clone();
    for src in &sources[1..] {
        result.bitxor_assign(src);
    }
    result
}

/// ANDOR: (src[1] | src[2] | ...) & src[0]
pub fn op_andor<T: RoaringType>(sources: Vec<T>) -> T {
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
pub fn op_andnot<T: RoaringType>(sources: Vec<T>) -> T {
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
pub fn op_ornot<T: RoaringType>(sources: Vec<T>) -> T {
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
pub fn op_one<T: RoaringType>(sources: Vec<T>) -> T {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::xorshift;
    use roaring::{RoaringBitmap, RoaringTreemap};
    use std::collections::BTreeSet;

    const OPS: [&str; 7] = ["AND", "OR", "XOR", "ANDOR", "DIFF", "DIFF1", "ONE"];

    fn build<T: RoaringType>(vals: &[u64]) -> T {
        let converted: Vec<T::Value> = vals
            .iter()
            .map(|&v| T::Value::try_from(v).ok().unwrap())
            .collect();
        T::from_values(&converted)
    }

    fn run_op<T: RoaringType>(op: &str, sources: Vec<T>) -> T {
        match op {
            "AND" => op_and(sources),
            "OR" => op_or(sources),
            "XOR" => op_xor(sources),
            "ANDOR" => op_andor(sources),
            "DIFF" => op_andnot(sources),
            "DIFF1" => op_ornot(sources),
            "ONE" => op_one(sources),
            _ => unreachable!(),
        }
    }

    /// Naive reference: per-element membership counting.
    fn reference(op: &str, sources: &[BTreeSet<u64>]) -> BTreeSet<u64> {
        let universe: BTreeSet<u64> = sources.iter().flatten().copied().collect();
        universe
            .into_iter()
            .filter(|v| {
                let count = sources.iter().filter(|s| s.contains(v)).count();
                let in_first = sources[0].contains(v);
                let in_rest = sources[1..].iter().any(|s| s.contains(v));
                match op {
                    "AND" => count == sources.len(),
                    "OR" => count > 0,
                    "XOR" => count % 2 == 1,
                    "ANDOR" => in_first && in_rest,
                    "DIFF" => in_first && !in_rest,
                    "DIFF1" => in_rest && !in_first,
                    "ONE" => count == 1,
                    _ => unreachable!(),
                }
            })
            .collect()
    }

    fn check_all_ops<T: RoaringType>(sets: &[BTreeSet<u64>]) {
        for op in OPS {
            let sources: Vec<T> = sets
                .iter()
                .map(|s| build::<T>(&s.iter().copied().collect::<Vec<_>>()))
                .collect();
            let result = run_op(op, sources);
            let expected = build::<T>(&reference(op, sets).into_iter().collect::<Vec<_>>());
            assert_eq!(result, expected, "op {} on sources {:?}", op, sets);
        }
    }

    #[test]
    fn bitop_kernels_match_reference_randomized() {
        let mut state = 0x243F_6A88_85A3_08D3u64;
        for _ in 0..80 {
            let n_sources = 1 + (xorshift(&mut state) % 4) as usize;
            let sets: Vec<BTreeSet<u64>> = (0..n_sources)
                .map(|_| {
                    let card = xorshift(&mut state) % 48;
                    (0..card).map(|_| xorshift(&mut state) % 128).collect()
                })
                .collect();
            check_all_ops::<RoaringBitmap>(&sets);
            check_all_ops::<RoaringTreemap>(&sets);
        }
    }

    #[test]
    fn bitop_one_known_answer() {
        // {1,2} {2,3} {3,4}: 1 and 4 appear exactly once; 2 and 3 twice.
        let sources: Vec<RoaringBitmap> = vec![build(&[1, 2]), build(&[2, 3]), build(&[3, 4])];
        assert_eq!(op_one(sources), build::<RoaringBitmap>(&[1, 4]));
        // A bit in all three sources is not "exactly one".
        let sources: Vec<RoaringBitmap> = vec![build(&[7, 1]), build(&[7, 2]), build(&[7, 3])];
        assert_eq!(op_one(sources), build::<RoaringBitmap>(&[1, 2, 3]));
    }

    #[test]
    fn bitop_single_source_semantics() {
        let src = build::<RoaringBitmap>(&[1, 5, 9]);
        assert_eq!(op_and(vec![src.clone()]), src);
        assert_eq!(op_or(vec![src.clone()]), src);
        assert_eq!(op_xor(vec![src.clone()]), src);
        assert_eq!(op_andnot(vec![src.clone()]), src);
        assert_eq!(op_one(vec![src.clone()]), src);
        // ANDOR / DIFF1 need at least two sources.
        assert_eq!(op_andor(vec![src.clone()]), RoaringBitmap::new());
        assert_eq!(op_ornot(vec![src]), RoaringBitmap::new());
    }

    #[test]
    fn bitop_no_sources_yield_empty() {
        for op in OPS {
            let result: RoaringBitmap = run_op(op, vec![]);
            assert!(result.is_empty(), "op {} with no sources", op);
        }
    }

    #[test]
    fn variadic_op_classification() {
        for op in OPS {
            assert!(is_variadic_op(op), "{} must be variadic", op);
        }
        assert!(!is_variadic_op("NOT"));
        assert!(!is_variadic_op("FOO"));
        assert!(!is_variadic_op(""));
    }
}
