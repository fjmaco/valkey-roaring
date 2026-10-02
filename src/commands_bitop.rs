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
use crate::commands::{check_type, from_aof_or_primary, key_changed, parse_value};
use crate::error::*;
use crate::parse::c_str;
use valkey_module::native_types::ValkeyType;
use valkey_module::{Context, ValkeyError, ValkeyResult, ValkeyString, ValkeyValue};

/// Operations that take a destination plus one or more source keys. Names
/// match exactly, as upstream's strcmp does: no case folding.
pub fn is_variadic_op(op: &[u8]) -> bool {
    matches!(
        op,
        b"AND" | b"OR" | b"XOR" | b"ANDOR" | b"DIFF" | b"DIFF1" | b"ONE"
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
    let op = c_str(args[1].as_slice());
    if op == b"NOT" {
        if args.len() <= 5 {
            ctx.key_at_pos(2);
            ctx.key_at_pos(3);
        }
    } else if is_variadic_op(op) && args.len() >= 5 {
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

    let mut op = c_str(args[1].as_slice());
    // 1.1.1 matched operation names in any case: replayed or replicated
    // commands may carry a lowercase one (see from_aof_or_primary).
    let legacy_op;
    if op != b"NOT" && !is_variadic_op(op) && from_aof_or_primary(ctx) {
        legacy_op = String::from_utf8_lossy(args[1].as_slice()).to_uppercase();
        op = legacy_op.as_bytes();
    }

    if op == b"NOT" {
        return handle_bitop_not::<T>(ctx, &args, vtype);
    }
    if !is_variadic_op(op) {
        return Err(ValkeyError::Str(ERR_SYNTAX));
    }
    // Variadic operations need at least two sources (redis-roaring arity).
    if args.len() < 5 {
        return Err(ValkeyError::WrongArity);
    }
    // Upstream checks the destination's type before the sources'.
    check_type::<T>(ctx, &args[2], vtype)?;

    // Borrow every source in place rather than copying it. The read handles
    // close at the end of this block, before the destination opens, so the
    // destination may also be a source.
    let mut result = {
        let empty = T::new();
        let keys: Vec<_> = args[3..].iter().map(|arg| ctx.open_key(arg)).collect();
        let mut sources: Vec<&T> = Vec::with_capacity(keys.len());
        for key in &keys {
            sources.push(key.get_value::<T>(vtype)?.unwrap_or(&empty));
        }
        match op {
            b"AND" => op_and(&sources),
            b"OR" => op_or(&sources),
            b"XOR" => op_xor(&sources),
            b"ANDOR" => op_andor(&sources),
            b"DIFF" => op_andnot(&sources),
            b"DIFF1" => op_ornot(&sources),
            b"ONE" => op_one(&sources),
            _ => unreachable!(),
        }
    };
    result.trim();

    let cardinality = result.len() as i64;
    let dest = ctx.open_key_writable(&args[2]);
    dest.set_value(vtype, result)?;
    key_changed(ctx, &args[2]);

    Ok(ValkeyValue::Integer(cardinality))
}

/// NOT: complement of a single source within the universe [0, last].
///
/// Without an explicit `last` the universe ends at the source's max value; an
/// explicit `last` below the source max is raised to it. A missing or empty
/// source with no `last` stores an empty bitmap and replies 0; with `last` it
/// stores the full range [0, last]. A universe of more than MAX_STORED_RANGE
/// values (only reachable with 64-bit values) is refused before anything is
/// built: complementing [0, 2^63] would allocate billions of containers.
fn handle_bitop_not<T: RoaringType>(
    ctx: &Context,
    args: &[ValkeyString],
    vtype: &ValkeyType,
) -> ValkeyResult {
    // Upstream's order: `last`, then the destination's type, then the source.
    let last = match args.len() {
        4 => None,
        5 => Some(parse_value::<T>(ctx, &args[4], "last")?),
        _ => return Err(ValkeyError::WrongArity),
    };
    check_type::<T>(ctx, &args[2], vtype)?;

    // Borrowed source, closed before the destination opens (see above).
    let result = {
        let empty = T::new();
        let key = ctx.open_key(&args[3]);
        let src = key.get_value::<T>(vtype)?.unwrap_or(&empty);
        let universe_max = match (src.max_val(), last) {
            (None, None) => None,
            (None, Some(last)) => Some(last),
            (Some(max), None) => Some(max),
            (Some(max), Some(last)) => Some(if last > max { last } else { max }),
        };
        match universe_max {
            None => T::new(),
            Some(top) if T::value_to_u64(top) >= MAX_STORED_RANGE => {
                return Err(ValkeyError::Str(ERR_STORED_RANGE_TOO_LARGE));
            }
            Some(top) => src.flip_inclusive(top),
        }
    };
    // No trim() here, unlike the other operations: a complement cannot
    // inherit an input's capacity (the case trim exists for); its slack is
    // run vectors grown by doubling, as 1.1.1 kept them. The statistics pass
    // and the compacting copy cost a NOT 4-9% (35.9 vs 37.3 us on a
    // 2,000-value clustered key, 564 vs 620 us on 50,000 sparse values).

    let cardinality = result.len() as i64;
    let dest = ctx.open_key_writable(&args[2]);
    dest.set_value(vtype, result)?;
    key_changed(ctx, &args[2]);

    Ok(ValkeyValue::Integer(cardinality))
}

// The kernels take borrowed sources. The first two combine into a fresh set
// (no copy of either), and the rest fold into it in place.

/// AND: intersection of all sources.
pub fn op_and<T: RoaringType>(sources: &[&T]) -> T {
    match sources {
        [] => T::new(),
        [only] => (*only).clone(),
        [first, second, rest @ ..] => {
            let mut result = first.intersection(second);
            for src in rest {
                result.bitand_assign(src);
            }
            result
        }
    }
}

/// OR: union of all sources.
pub fn op_or<T: RoaringType>(sources: &[&T]) -> T {
    match sources {
        [] => T::new(),
        [only] => (*only).clone(),
        [first, second, rest @ ..] => {
            let mut result = first.union(second);
            for src in rest {
                result.bitor_assign(src);
            }
            result
        }
    }
}

/// XOR: symmetric difference of all sources.
pub fn op_xor<T: RoaringType>(sources: &[&T]) -> T {
    match sources {
        [] => T::new(),
        [only] => (*only).clone(),
        [first, second, rest @ ..] => {
            let mut result = first.symmetric_difference(second);
            for src in rest {
                result.bitxor_assign(src);
            }
            result
        }
    }
}

/// ANDOR: (src[1] | src[2] | ...) & src[0]
pub fn op_andor<T: RoaringType>(sources: &[&T]) -> T {
    match sources {
        [] | [_] => T::new(),
        // One source on the right: a plain intersection, which can build
        // its result fresh instead of copying that source.
        [first, only] => only.intersection(first),
        [first, rest @ ..] => {
            let mut union = op_or(rest);
            union.bitand_assign(first);
            union
        }
    }
}

/// ANDNOT / DIFF: src[0] - src[1] - src[2] - ...
pub fn op_andnot<T: RoaringType>(sources: &[&T]) -> T {
    match sources {
        [] => T::new(),
        [only] => (*only).clone(),
        [first, second, rest @ ..] => {
            let mut result = first.difference(second);
            for src in rest {
                result.sub_assign(src);
            }
            result
        }
    }
}

/// ORNOT / DIFF1: (src[1] | src[2] | ...) - src[0]
pub fn op_ornot<T: RoaringType>(sources: &[&T]) -> T {
    match sources {
        [] | [_] => T::new(),
        [first, only] => only.difference(first),
        [first, rest @ ..] => {
            let mut union = op_or(rest);
            union.sub_assign(first);
            union
        }
    }
}

/// ONE: bits present in exactly one source.
/// Algorithm: XOR accumulator + intersection tracker to remove duplicates.
pub fn op_one<T: RoaringType>(sources: &[&T]) -> T {
    match sources {
        [] => T::new(),
        [only] => (*only).clone(),
        [first, rest @ ..] => {
            // `result` tracks XOR accumulator (bits toggled odd number of times)
            // `seen_twice` tracks bits that appeared in 2+ sources
            let mut result = (*first).clone();
            let mut seen_twice = T::new();
            for src in rest {
                // Bits in both result and src were already in some source +
                // this source → duplicates
                seen_twice.bitor_assign_owned(result.intersection(src));
                result.bitxor_assign(src);
            }
            // Remove all bits that appeared in 2+ sources
            result.sub_assign(&seen_twice);
            result
        }
    }
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
        T::from_values(converted)
    }

    fn run_op<T: RoaringType>(op: &str, sources: Vec<T>) -> T {
        let sources: Vec<&T> = sources.iter().collect();
        let sources = &sources[..];
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
        assert_eq!(run_op("ONE", sources), build::<RoaringBitmap>(&[1, 4]));
        // A bit in all three sources is not "exactly one".
        let sources: Vec<RoaringBitmap> = vec![build(&[7, 1]), build(&[7, 2]), build(&[7, 3])];
        assert_eq!(run_op("ONE", sources), build::<RoaringBitmap>(&[1, 2, 3]));
    }

    #[test]
    fn bitop_single_source_semantics() {
        let src = build::<RoaringBitmap>(&[1, 5, 9]);
        for op in ["AND", "OR", "XOR", "DIFF", "ONE"] {
            assert_eq!(run_op(op, vec![src.clone()]), src, "op {}", op);
        }
        // ANDOR / DIFF1 need at least two sources.
        assert_eq!(run_op("ANDOR", vec![src.clone()]), RoaringBitmap::new());
        assert_eq!(run_op("DIFF1", vec![src]), RoaringBitmap::new());
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
            assert!(is_variadic_op(op.as_bytes()), "{} must be variadic", op);
            // Exact names only, as upstream's strcmp.
            let lower = op.to_lowercase();
            assert!(
                !is_variadic_op(lower.as_bytes()),
                "{} must not match",
                lower
            );
        }
        assert!(!is_variadic_op(b"NOT"));
        assert!(!is_variadic_op(b"FOO"));
        assert!(!is_variadic_op(b""));
        assert!(!is_variadic_op(b"And"));
    }
}
