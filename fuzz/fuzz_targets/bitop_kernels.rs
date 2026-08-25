//! Fuzzes the seven BITOP kernels against a naive per-element membership
//! reference over byte-driven source sets.
#![no_main]

use libfuzzer_sys::fuzz_target;
use roaring::RoaringBitmap;
use std::collections::BTreeSet;
use valkey_roaring::fuzzing::{op_and, op_andnot, op_andor, op_one, op_or, op_ornot, op_xor};

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }
    let n = (data[0] % 4) as usize + 1;
    let mut sets: Vec<BTreeSet<u32>> = vec![BTreeSet::new(); n];
    for c in data[1..].chunks_exact(2) {
        sets[(c[0] as usize) % n].insert(c[1] as u32);
    }
    let sources: Vec<RoaringBitmap> = sets.iter().map(|s| s.iter().copied().collect()).collect();

    let universe: BTreeSet<u32> = sets.iter().flatten().copied().collect();
    let count = |v: u32| sets.iter().filter(|s| s.contains(&v)).count();
    let in_first = |v: u32| sets[0].contains(&v);
    let in_rest = |v: u32| sets[1..].iter().any(|s| s.contains(&v));

    let check = |name: &str, result: RoaringBitmap, pred: &dyn Fn(u32) -> bool| {
        let expected: RoaringBitmap = universe.iter().copied().filter(|v| pred(*v)).collect();
        assert_eq!(result, expected, "op {} over {:?}", name, sets);
    };

    check("AND", op_and(sources.clone()), &|v| count(v) == n);
    check("OR", op_or(sources.clone()), &|v| count(v) > 0);
    check("XOR", op_xor(sources.clone()), &|v| count(v) % 2 == 1);
    check("ONE", op_one(sources.clone()), &|v| count(v) == 1);
    check("DIFF", op_andnot(sources.clone()), &|v| {
        in_first(v) && !in_rest(v)
    });
    // ANDOR / DIFF1 with a single source are defined as empty; the reference
    // agrees because in_rest is always false there.
    check("ANDOR", op_andor(sources.clone()), &|v| {
        n >= 2 && in_first(v) && in_rest(v)
    });
    check("DIFF1", op_ornot(sources), &|v| {
        n >= 2 && in_rest(v) && !in_first(v)
    });
});
