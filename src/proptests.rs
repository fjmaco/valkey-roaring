//! Cross-cutting property tests: serialization round-trips, robustness of the
//! R.IMPORT deserialization path against malformed bytes, and R-vs-R64 parity.

use crate::bitmap_type::RoaringType;
use crate::test_util::xorshift;
use roaring::{RoaringBitmap, RoaringTreemap};
use std::io::Cursor;

fn random_bitmap32(state: &mut u64, max_card: u64, span: u64) -> RoaringBitmap {
    let card = xorshift(state) % max_card;
    (0..card).map(|_| (xorshift(state) % span) as u32).collect()
}

fn random_bitmap64(state: &mut u64, max_card: u64) -> RoaringTreemap {
    let card = xorshift(state) % max_card;
    (0..card)
        .map(|_| {
            // Mix small values and values above the u32 range.
            let v = xorshift(state);
            if v.is_multiple_of(3) {
                v
            } else {
                v % 100_000
            }
        })
        .collect()
}

#[test]
fn serialize_round_trip_32() {
    let mut state = 0x1234_5678_9ABC_DEF1u64;
    for _ in 0..25 {
        let b = random_bitmap32(&mut state, 500, 1 << 20);
        let mut buf = Vec::new();
        RoaringType::serialize_into(&b, &mut buf).unwrap();
        assert_eq!(buf.len(), RoaringType::serialized_size(&b));
        let back = <RoaringBitmap as RoaringType>::deserialize_from(Cursor::new(buf)).unwrap();
        assert_eq!(b, back);
    }
}

#[test]
fn serialize_round_trip_64() {
    let mut state = 0xFEDC_BA98_7654_3210u64;
    for _ in 0..25 {
        let b = random_bitmap64(&mut state, 500);
        let mut buf = Vec::new();
        RoaringType::serialize_into(&b, &mut buf).unwrap();
        let back = <RoaringTreemap as RoaringType>::deserialize_from(Cursor::new(buf)).unwrap();
        assert_eq!(b, back);
    }
}

/// The R.IMPORT path feeds untrusted network bytes into deserialize_from.
/// Corrupted, truncated, or garbage input must return Err — never panic,
/// because a panic across the module FFI boundary aborts the whole server.
#[test]
fn deserialize_malformed_bytes_never_panics() {
    let mut state = 0xDEAD_BEEF_CAFE_F00Du64;

    let mut bases: Vec<Vec<u8>> = Vec::new();
    let b32: RoaringBitmap = (0..1000u32).filter(|v| v % 3 == 0).collect();
    let mut buf = Vec::new();
    RoaringType::serialize_into(&b32, &mut buf).unwrap();
    bases.push(buf);
    let b64: RoaringTreemap = (0..1000u64).map(|v| v * (1 << 33)).collect();
    let mut buf = Vec::new();
    RoaringType::serialize_into(&b64, &mut buf).unwrap();
    bases.push(buf);

    let check = |bytes: Vec<u8>, what: &str| {
        let cloned = bytes.clone();
        let result = std::panic::catch_unwind(move || {
            let _ = <RoaringBitmap as RoaringType>::deserialize_from(Cursor::new(&cloned[..]));
            let _ = <RoaringTreemap as RoaringType>::deserialize_from(Cursor::new(&cloned[..]));
        });
        assert!(
            result.is_ok(),
            "deserialize panicked on {}: {:?}",
            what,
            bytes
        );
    };

    // Pure garbage of various lengths.
    for len in [0usize, 1, 4, 8, 17, 64, 1024] {
        let bytes: Vec<u8> = (0..len).map(|_| xorshift(&mut state) as u8).collect();
        check(bytes, "garbage");
    }

    // Corruptions of valid serializations: single-byte flips and truncations.
    for base in &bases {
        for _ in 0..400 {
            let mut bytes = base.clone();
            match xorshift(&mut state) % 3 {
                0 => {
                    let idx = (xorshift(&mut state) as usize) % bytes.len();
                    bytes[idx] ^= (1 + xorshift(&mut state) % 255) as u8;
                }
                1 => {
                    bytes.truncate((xorshift(&mut state) as usize) % bytes.len());
                }
                _ => {
                    let idx = (xorshift(&mut state) as usize) % bytes.len();
                    bytes[idx] = 0xFF;
                    bytes.truncate(idx + 1 + (xorshift(&mut state) as usize) % (bytes.len() - idx));
                }
            }
            check(bytes, "mutation");
        }
    }
}

/// The 32-bit and 64-bit command families must behave identically for values
/// within the u32 range (mirrors upstream's fuzz_r_vs_r64_parity gate).
#[test]
fn r_vs_r64_parity_random_ops() {
    let mut state = 0xB529_7A4D_3F84_D5B5u64;
    for _ in 0..40 {
        let mut b32 = RoaringBitmap::new();
        let mut b64 = RoaringTreemap::new();
        for _ in 0..300 {
            let v = (xorshift(&mut state) % 1024) as u32;
            match xorshift(&mut state) % 4 {
                0 => assert_eq!(b32.insert(v), b64.insert(v as u64)),
                1 => assert_eq!(b32.remove(v), b64.remove(v as u64)),
                2 => assert_eq!(b32.contains(v), b64.contains(v as u64)),
                _ => assert_eq!(
                    RoaringType::nth_absent(&b32, 1).map(u64::from),
                    RoaringType::nth_absent(&b64, 1)
                ),
            }
        }
        assert_eq!(b32.len(), b64.len());
        assert_eq!(b32.min().map(u64::from), b64.min());
        assert_eq!(b32.max().map(u64::from), b64.max());

        let f32v = RoaringType::flip_inclusive(&b32, 2048);
        let f64v = RoaringType::flip_inclusive(&b64, 2048);
        assert_eq!(
            f32v.iter().map(u64::from).collect::<Vec<_>>(),
            f64v.iter().collect::<Vec<_>>()
        );

        let mut s32 = Vec::new();
        let mut s64 = Vec::new();
        RoaringType::serialize_into(&b32, &mut s32).unwrap();
        RoaringType::serialize_into(&b64, &mut s64).unwrap();
        // Formats differ, but both must round-trip to the same logical set.
        let r32 = <RoaringBitmap as RoaringType>::deserialize_from(Cursor::new(s32)).unwrap();
        let r64 = <RoaringTreemap as RoaringType>::deserialize_from(Cursor::new(s64)).unwrap();
        assert_eq!(
            r32.iter().map(u64::from).collect::<Vec<_>>(),
            r64.iter().collect::<Vec<_>>()
        );
    }
}

fn bytes<T: RoaringType>(b: &T) -> Vec<u8> {
    let mut buf = Vec::new();
    b.serialize_into(&mut buf).unwrap();
    buf
}

/// Portable-format bytes for one container (key 0) that writes `vals` as an
/// array container whatever its size, the way Go roaring v1.9.4's
/// FastOr/ParOr serializes some unions. Readers infer the container type
/// from the cardinality, so above 4096 values they parse an 8 KiB bitmap
/// container instead and the stream misaligns.
fn array_container_blob(vals: &[u16]) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&12346u32.to_le_bytes()); // SERIAL_COOKIE_NO_RUNCONTAINER
    b.extend_from_slice(&1u32.to_le_bytes()); // container count
    b.extend_from_slice(&0u16.to_le_bytes()); // container key
    b.extend_from_slice(&((vals.len() - 1) as u16).to_le_bytes()); // cardinality - 1
    b.extend_from_slice(&16u32.to_le_bytes()); // offset of the container data
    for v in vals {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b
}

/// The same container wrapped as a one-bitmap 64-bit (treemap) blob.
fn array_container_blob64(vals: &[u16]) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&1u64.to_le_bytes()); // sub-bitmap count
    b.extend_from_slice(&0u32.to_le_bytes()); // high 32 bits
    b.extend(array_container_blob(vals));
    b
}

/// R.IMPORT must refuse oversized array containers (a known producer bug)
/// rather than store whatever set the misaligned stream decodes to.
#[test]
fn oversized_array_container_blob_is_rejected() {
    // At the 4096-value limit the layout is valid and round-trips.
    let at_limit: Vec<u16> = (0..4096u16).map(|i| i * 16).collect();
    let b32 = <RoaringBitmap as RoaringType>::deserialize_from(Cursor::new(array_container_blob(
        &at_limit,
    )))
    .unwrap();
    assert_eq!(b32.iter().map(|v| v as u16).collect::<Vec<_>>(), at_limit);
    let b64 = <RoaringTreemap as RoaringType>::deserialize_from(Cursor::new(
        array_container_blob64(&at_limit),
    ))
    .unwrap();
    assert_eq!(b64.len(), 4096);

    for n in [4097usize, 5000, 10_000, 65_535] {
        let vals: Vec<u16> = (0..n).map(|i| (i * 65_536 / n) as u16).collect();
        assert!(
            <RoaringBitmap as RoaringType>::deserialize_from(Cursor::new(array_container_blob(
                &vals
            )))
            .is_err(),
            "32-bit accepted an array container of {n} values"
        );
        assert!(
            <RoaringTreemap as RoaringType>::deserialize_from(Cursor::new(array_container_blob64(
                &vals
            )))
            .is_err(),
            "64-bit accepted an array container of {n} values"
        );
    }
}

/// SETRANGE's no-op check: R and R64 must agree with a brute-force answer,
/// including ranges that straddle R64's 32-bit sub-bitmap boundaries.
#[test]
fn contains_range_matches_reference() {
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    const SPAN: u32 = 3000;
    for _ in 0..200 {
        // Long runs with holes, so ranges are often (but not always) present.
        let mut b32 = RoaringBitmap::new();
        for _ in 0..4 {
            let s = (xorshift(&mut state) % u64::from(SPAN)) as u32;
            let len = (xorshift(&mut state) % 1200) as u32;
            b32.insert_range(s..(s + len).min(SPAN));
        }
        for _ in 0..3 {
            b32.remove((xorshift(&mut state) % u64::from(SPAN)) as u32);
        }
        for base in [0u64, (1 << 32) - 1500] {
            let b64: RoaringTreemap = b32.iter().map(|v| base + u64::from(v)).collect();
            for _ in 0..20 {
                let s = (xorshift(&mut state) % u64::from(SPAN)) as u32;
                let e = (xorshift(&mut state) % u64::from(SPAN)) as u32;
                let expected = (s..e).all(|v| b32.contains(v));
                assert_eq!(b32.contains_range_exclusive(s, e), expected, "R {s}..{e}");
                assert_eq!(
                    b64.contains_range_exclusive(base + u64::from(s), base + u64::from(e)),
                    expected,
                    "R64 {s}..{e} at base {base}"
                );
            }
        }
    }

    // A range spanning three sub-bitmaps, the middle one full.
    let (start, end) = ((1u64 << 32) - 10, (2u64 << 32) + 10);
    let mut wide = RoaringTreemap::new();
    wide.insert_range(start..end);
    assert!(wide.contains_range_exclusive(start, end));
    assert!(wide.contains_range_exclusive(start + 5, end - 5));
    assert!(!wide.contains_range_exclusive(start - 1, end));
    assert!(!wide.contains_range_exclusive(start, end + 1));
    wide.remove(1u64 << 32 | 12_345);
    assert!(!wide.contains_range_exclusive(start, end));
    assert!(RoaringTreemap::new().contains_range_exclusive(7, 7));
    assert!(!RoaringTreemap::new().contains_range_exclusive(7, 8));
}

/// Handlers skip replication when a write changes nothing; such a write
/// should leave the stored value untouched, layout included.
#[test]
fn noop_writes_preserve_layout() {
    // A full container stored as a bitset (inserts never form runs), and a
    // mix of array, bitset and run containers.
    let mut full_bitset = RoaringBitmap::new();
    for v in 0..65_536u32 {
        full_bitset.insert(v);
    }
    let mut mixed: RoaringBitmap = (0..200_000u32).filter(|v| v % 7 != 0).collect();
    mixed.insert_range(300_000..400_000);
    mixed.extend((500_000..520_000u32).step_by(13));
    mixed.optimize();

    for b32 in [full_bitset.clone(), mixed] {
        let present: Vec<u32> = b32.iter().step_by(97).collect();
        let absent: Vec<u32> = (0..600_000u32)
            .step_by(89)
            .filter(|v| !b32.contains(*v))
            .collect();
        let b64: RoaringTreemap = b32.iter().map(u64::from).collect();
        let (present64, absent64): (Vec<u64>, Vec<u64>) = (
            present.iter().copied().map(u64::from).collect(),
            absent.iter().copied().map(u64::from).collect(),
        );

        let mut b = b32.clone();
        assert!(!RoaringType::insert(&mut b, present[0]));
        assert!(!RoaringType::remove(&mut b, absent[0]));
        assert_eq!(b.insert_many(&present), 0);
        assert_eq!(b.remove_many(&absent), 0);
        assert_eq!(bytes(&b), bytes(&b32));

        let mut b = b64.clone();
        assert_eq!(b.insert_many(&present64), 0);
        assert_eq!(b.remove_many(&absent64), 0);
        assert_eq!(bytes(&b), bytes(&b64));
    }

    // Why SETRANGE tests containment instead of the inserted count: an
    // already-full range adds nothing yet re-shapes the bitset into a run.
    let mut b = full_bitset.clone();
    assert!(b.contains_range_exclusive(0, 65_536));
    assert_eq!(b.insert_range_exclusive(0, 65_536), 0);
    assert_ne!(bytes(&b), bytes(&full_bitset));
}

/// Bulk ops report what changed (drives replication) and container counts
/// feed free_effort (drives lazy freeing of big values).
#[test]
fn change_counts_and_container_counts() {
    let mut b32 = RoaringBitmap::new();
    assert_eq!(b32.insert_many(&[1, 2, 2, 3, 70_000, 140_000]), 5);
    assert_eq!(b32.insert_many(&[1, 2, 3]), 0);
    assert_eq!(b32.container_count(), 3);
    assert_eq!(b32.remove_many(&[2, 4, 70_000, 70_000]), 2);
    assert_eq!(b32.container_count(), 2);

    let mut b64 = RoaringTreemap::new();
    assert_eq!(
        b64.insert_many(&[1, 70_000, 1 << 40, (1 << 40) + 70_000]),
        4
    );
    assert_eq!(b64.container_count(), 4);
    assert_eq!(b64.remove_many(&[1, 1, 5]), 1);
    assert_eq!(b64.container_count(), 3);
    assert_eq!(RoaringTreemap::new().container_count(), 0);
}

/// RANGEINTARRAY pages with one select plus an ascending walk; the reply must
/// equal the former select-per-position loop, for both widths.
#[test]
fn range_page_matches_select_per_position() {
    fn page<T: RoaringType>(b: &T, start: u64, end: u64) -> Vec<T::Value> {
        let card = b.len();
        if start >= card {
            return Vec::new();
        }
        let count = (end - start + 1).min(card - start) as usize;
        let first = b.select(start).unwrap();
        b.iter_from(first).take(count).collect()
    }
    fn reference<T: RoaringType>(b: &T, start: u64, end: u64) -> Vec<T::Value> {
        (start..=end).map_while(|i| b.select(i)).collect()
    }

    let mut state = 0x2545_F491_4F6C_DD1Du64;
    for _ in 0..60 {
        let b32 = random_bitmap32(&mut state, 3000, 1 << 21);
        let b64 = random_bitmap64(&mut state, 3000);
        for _ in 0..20 {
            let start = xorshift(&mut state) % 3200;
            let end = start + xorshift(&mut state) % 1500;
            assert_eq!(page(&b32, start, end), reference(&b32, start, end));
            assert_eq!(page(&b64, start, end), reference(&b64, start, end));
        }
    }
}

/// Bulk construction paths (sorted append) must build the same set as
/// inserting one value at a time, for both widths.
#[test]
fn bulk_construction_matches_inserts() {
    let mut state = 0x9FB2_1C65_1E98_DF25u64;
    for _ in 0..40 {
        let bits: Vec<u8> = (0..xorshift(&mut state) % 200_000)
            .map(|_| {
                if xorshift(&mut state).is_multiple_of(3) {
                    b'1'
                } else {
                    b'0'
                }
            })
            .collect();
        let mut by_insert = RoaringBitmap::new();
        for (i, &b) in bits.iter().enumerate() {
            if b == b'1' {
                by_insert.insert(i as u32);
            }
        }
        assert_eq!(
            <RoaringBitmap as RoaringType>::from_bit_array(&bits),
            by_insert
        );
        let b64 = <RoaringTreemap as RoaringType>::from_bit_array(&bits);
        assert!(b64.iter().eq(by_insert.iter().map(u64::from)));
    }
}

/// R.EXPORT's contract: one logical set, one blob, whatever history built
/// it. Sets made of short runs hit the case optimize() alone leaves open
/// (run and array encodings of equal size), so build each set five ways.
#[test]
fn export_is_canonical_across_histories() {
    fn histories<T: RoaringType>(runs: &[(u64, u64)], noise: &[u64]) -> Vec<T> {
        let v = |x: u64| T::Value::try_from(x).ok().unwrap();
        let values: Vec<T::Value> = runs.iter().flat_map(|&(s, l)| (s..s + l).map(v)).collect();
        let mut out = Vec::new();
        // 1. bulk build (array / bitset containers)
        out.push(T::from_values(values.clone()));
        // 2. range inserts (run containers)
        let mut b = T::new();
        for &(s, l) in runs {
            b.insert_range_exclusive(v(s), v(s + l));
        }
        out.push(b);
        // 3. superset, optimized, then the noise removed again
        let mut b = T::from_values(values.clone());
        b.insert_many(&noise.iter().map(|&x| v(x)).collect::<Vec<_>>());
        b.optimize();
        let extra: Vec<T::Value> = noise
            .iter()
            .map(|&x| v(x))
            .filter(|x| !values.contains(x))
            .collect();
        b.remove_many(&extra);
        out.push(b);
        // 4. one wide range per run group, optimized, then holes punched
        let mut b = T::new();
        if let (Some(&(lo, _)), Some(&(s, l))) = (runs.first(), runs.last()) {
            b.insert_range_exclusive(v(lo), v(s + l));
            b.optimize();
            let holes: Vec<T::Value> = (lo..s + l).map(v).filter(|x| !values.contains(x)).collect();
            b.remove_many(&holes);
        }
        out.push(b);
        // 5. one value at a time, back to front
        let mut b = T::new();
        for &x in values.iter().rev() {
            b.insert(x);
        }
        out.push(b);
        out
    }
    fn check<T: RoaringType>(runs: &[(u64, u64)], noise: &[u64]) {
        let mut blobs = Vec::new();
        for mut b in histories::<T>(runs, noise) {
            blobs.push(b.export_canonical().unwrap());
            // Exporting again (now optimized) must not change the bytes.
            assert_eq!(&b.export_canonical().unwrap(), blobs.last().unwrap());
        }
        for (i, blob) in blobs.iter().enumerate() {
            assert_eq!(blob, &blobs[0], "history {} differs for runs {:?}", i, runs);
        }
        let back = T::deserialize_from(Cursor::new(&blobs[0])).unwrap();
        let expected: u64 = runs.iter().map(|&(_, l)| l).sum();
        assert_eq!(back.len(), expected);
    }

    let mut state = 0xC2B2_AE3D_27D4_EB4Fu64;
    for _ in 0..300 {
        // Disjoint ascending runs of 1-4 values with gaps >= 1 inside a few
        // containers, so cardinality == 2 * runs + 1 happens often.
        let n_runs = 1 + xorshift(&mut state) % 6;
        let mut base = ((xorshift(&mut state) % 3) << 16) | (xorshift(&mut state) % 60_000);
        let mut runs = Vec::new();
        for _ in 0..n_runs {
            let len = 1 + xorshift(&mut state) % 4;
            runs.push((base, len));
            base += len + 1 + xorshift(&mut state) % 3;
        }
        let noise: Vec<u64> = (0..5)
            .map(|_| runs[0].0 + xorshift(&mut state) % 40)
            .collect();
        check::<RoaringBitmap>(&runs, &noise);
        // Same shapes for R64, shifted across a sub-bitmap border.
        let shift = (1u64 << 32) - 30_000;
        let runs64: Vec<(u64, u64)> = runs.iter().map(|&(s, l)| (s + shift, l)).collect();
        let noise64: Vec<u64> = noise.iter().map(|&x| x + shift).collect();
        check::<RoaringTreemap>(&runs64, &noise64);
    }
}

/// trim() never changes the set, and reclaims the slack a sparse AND result
/// inherits from its inputs (roaring-rs sizes it for the larger input).
#[test]
fn trim_reclaims_set_operation_slack() {
    // 200 pairs of 1000-value array containers overlapping in 10 values.
    let a: RoaringBitmap = (0..200u32)
        .flat_map(|c| (0..1000u32).map(move |i| (c << 16) | (i * 16)))
        .collect();
    let b: RoaringBitmap = (0..200u32)
        .flat_map(|c| (0..1000u32).map(move |i| (c << 16) | (i * 16 + u32::from(i % 100 != 0))))
        .collect();
    let mut and = RoaringType::intersection(&a, &b);
    let before = and.heap_size();
    let copy = and.clone();
    and.trim();
    assert_eq!(and, copy);
    assert!(
        and.heap_size() * 4 < before,
        "{} vs {}",
        and.heap_size(),
        before
    );

    let mut state = 0x6A09_E667_F3BC_C909u64;
    for _ in 0..20 {
        let x = random_bitmap32(&mut state, 5000, 1 << 22);
        let y = random_bitmap32(&mut state, 5000, 1 << 22);
        for mut r in [
            x.union(&y),
            x.intersection(&y),
            x.difference(&y),
            x.flip_inclusive(1 << 20),
        ] {
            let copy = r.clone();
            r.trim();
            assert_eq!(r, copy);
        }
        let x64 = random_bitmap64(&mut state, 3000);
        let mut r = x64.symmetric_difference(&random_bitmap64(&mut state, 3000));
        let copy = r.clone();
        r.trim();
        assert_eq!(r, copy);
    }
}

// ------------------------------------------------------------------
// QA edge-case tests (independent review of the optimization pass)
// ------------------------------------------------------------------

/// iter_from (RANGEINTARRAY's walk) must yield exactly the values >= start,
/// for any start: present or absent, on a container or sub-bitmap border,
/// past the maximum, at the top of the type.
#[test]
fn iter_from_matches_naive_filter() {
    let mut state = 0xD1B5_4A32_D192_ED03u64;
    for _ in 0..40 {
        let b32 = random_bitmap32(&mut state, 4000, 1 << 20);
        let mut b64 = random_bitmap64(&mut state, 4000);
        // Values on both sides of several 2^32 borders and at the top.
        for hi in [1u64, 2, 7, (1 << 31), (1 << 32) - 1] {
            for d in [0u64, 1, 2, 65_535, 65_536] {
                b64.insert((hi << 32) | d);
                b64.insert((hi << 32).wrapping_sub(1 + d));
            }
        }
        b64.insert(u64::MAX);
        let mut starts32: Vec<u32> = vec![0, 1, 65_535, 65_536, u32::MAX];
        let mut starts64: Vec<u64> = vec![0, 1, u32::MAX as u64, 1 << 32, (1 << 32) + 1, u64::MAX];
        for _ in 0..30 {
            starts32.push((xorshift(&mut state) % (1 << 21)) as u32);
            starts64.push(xorshift(&mut state));
            if let Some(v) = b64.select(xorshift(&mut state) % b64.len()) {
                starts64.push(v);
                starts64.push(v.wrapping_add(1));
            }
        }
        for s in starts32 {
            let got: Vec<u32> = RoaringType::iter_from(&b32, s).collect();
            let want: Vec<u32> = b32.iter().filter(|&v| v >= s).collect();
            assert_eq!(got, want, "R start {s}");
        }
        for s in starts64 {
            let got: Vec<u64> = RoaringType::iter_from(&b64, s).collect();
            let want: Vec<u64> = b64.iter().filter(|&v| v >= s).collect();
            assert_eq!(got, want, "R64 start {s}");
        }
    }
}

/// BITPOS 0 walks runs; it must agree with a value-by-value scan, including
/// runs that cross container and sub-bitmap borders and runs that reach the
/// top of the type (where no absent value is left: None).
#[test]
fn nth_absent_matches_naive_scan() {
    /// The n-th absent value >= `from` (1-based), scanning value by value.
    fn naive32(b: &RoaringBitmap, n: u64, from: u64) -> Option<u32> {
        let mut left = n;
        (from..=u32::MAX as u64)
            .find(|&v| {
                if !b.contains(v as u32) {
                    left -= 1;
                }
                left == 0
            })
            .map(|v| v as u32)
    }
    let mut state = 0x8CB9_2BA7_2F3D_8DD7u64;
    for _ in 0..60 {
        // Ranges in a window at 0 or at the top, so a short scan suffices.
        let top = xorshift(&mut state).is_multiple_of(2);
        let base: u32 = if top { u32::MAX - 200_000 } else { 0 };
        let mut b = RoaringBitmap::new();
        for _ in 0..(xorshift(&mut state) % 6) {
            let s = base + (xorshift(&mut state) % 190_000) as u32;
            let l = (xorshift(&mut state) % 70_000) as u32;
            b.insert_range(s..=s.saturating_add(l));
        }
        if top && xorshift(&mut state).is_multiple_of(2) {
            b.insert_range(u32::MAX - 1000..=u32::MAX);
        }
        let offsets = [1u64, 2, 3, 17, 4096, 65_537, 150_000, 200_001];
        let ns: Vec<u64> = if top {
            std::iter::once(1)
                .chain(offsets.iter().map(|&d| base as u64 + d - 1))
                .collect()
        } else {
            offsets.to_vec()
        };
        for n in ns {
            let got = RoaringType::nth_absent(&b, n);
            // Everything below `base` is absent.
            let want = if n <= base as u64 {
                Some(n as u32 - 1)
            } else {
                naive32(&b, n - base as u64, base as u64)
            };
            assert_eq!(got, want, "R n={n} top={top}");
        }
        // R64: the window just below the first 2^32 border, so runs cross it.
        if !top {
            let shift = (1u64 << 32) - 100_000;
            let t: RoaringTreemap = b.iter().map(|v| shift + v as u64).collect();
            // [0, shift) is absent, so ask for the d-th absent value from
            // `shift` on: n = shift + d.
            for d in [1u64, 5, 99_999, 100_000, 100_001, 300_000] {
                let want = (shift..)
                    .filter(|v| !t.contains(*v))
                    .nth((d - 1) as usize)
                    .unwrap();
                assert_eq!(
                    RoaringType::nth_absent(&t, shift + d),
                    Some(want),
                    "R64 d={d}"
                );
            }
        }
    }
    // R64 at the top of u64: runs up to u64::MAX leave nothing above them.
    let mut t = RoaringTreemap::new();
    t.insert_range(u64::MAX - 70_000..=u64::MAX);
    t.insert_range(u64::MAX - 200_000..u64::MAX - 100_000);
    let absent = u64::MAX - 200_000; // [0, u64::MAX - 200_000) all absent
    assert_eq!(RoaringType::nth_absent(&t, 1), Some(0));
    assert_eq!(RoaringType::nth_absent(&t, absent), Some(absent - 1));
    assert_eq!(
        RoaringType::nth_absent(&t, absent + 1),
        Some(u64::MAX - 100_000)
    );
    let total_absent = absent + 100_000 - 70_000 - 1 + 1;
    assert_eq!(
        RoaringType::nth_absent(&t, total_absent),
        Some(u64::MAX - 70_001)
    );
    assert_eq!(RoaringType::nth_absent(&t, total_absent + 1), None);
    let full_top: RoaringTreemap = [u64::MAX].into_iter().collect();
    assert_eq!(
        RoaringType::nth_absent(&full_top, u64::MAX),
        Some(u64::MAX - 1)
    );
    assert_eq!(
        RoaringType::nth_absent(&full_top, u64::MAX - 1),
        Some(u64::MAX - 2)
    );
}

/// JACCARD's one-pass union (|A| + |B| - |A & B|) must equal roaring's
/// union_len for any pair, both widths.
#[test]
fn jaccard_union_formula_matches_union_len() {
    let mut state = 0x3C6E_F372_FE94_F82Bu64;
    for _ in 0..200 {
        let a = random_bitmap32(&mut state, 5000, 1 << 18);
        let b = random_bitmap32(&mut state, 5000, 1 << 18);
        let inter = RoaringType::intersection_len(&a, &b);
        assert_eq!(a.len() + b.len() - inter, a.union_len(&b));
        let a = random_bitmap64(&mut state, 3000);
        let b = random_bitmap64(&mut state, 3000);
        let inter = RoaringType::intersection_len(&a, &b);
        assert_eq!(a.len() + b.len() - inter, a.union_len(&b));
    }
    let full = RoaringBitmap::full();
    assert_eq!(
        full.len() + full.len() - RoaringType::intersection_len(&full, &full),
        1 << 32
    );
}

/// Canonical export at the edges canonical.rs computes positions for: a tie
/// in container 0xFFFF, in the top sub-bitmap of u64, beside bitset
/// containers, in blobs with fewer than four and with four or more
/// containers; histories include IMPORT-style deserialization of a
/// run-encoded blob, an owned union of halves and a double NOT.
#[test]
fn export_canonical_at_edge_positions() {
    fn tie(base: u64, runs: u64) -> Vec<u64> {
        let mut out = Vec::new();
        let mut pos = base + 10;
        for i in 0..runs {
            let n = if i == 0 { 3 } else { 2 };
            out.extend(pos..pos + n);
            pos += n + 3;
        }
        out
    }
    fn histories<T: RoaringType>(vals: &[u64]) -> Vec<T> {
        let v = |x: u64| T::Value::try_from(x).ok().unwrap();
        let typed: Vec<T::Value> = vals.iter().map(|&x| v(x)).collect();
        let mut out = vec![T::from_values(typed.clone())];
        // runs via range inserts (no shape below reaches the type's max)
        let mut b = T::new();
        let mut i = 0;
        while i < vals.len() {
            let mut j = i;
            while j + 1 < vals.len() && vals[j + 1] == vals[j] + 1 {
                j += 1;
            }
            b.insert_range_exclusive(v(vals[i]), v(vals[j] + 1));
            i = j + 1;
        }
        // a deserialized run-encoded blob (what IMPORT of such a blob builds)
        let mut blob = Vec::new();
        b.serialize_into(&mut blob).unwrap();
        out.push(T::deserialize_from(Cursor::new(blob)).unwrap());
        out.push(b);
        // first half bit by bit and optimized, then an owned union
        let half = typed.len() / 2;
        let mut lo = T::new();
        for &x in &typed[..half] {
            lo.insert(x);
        }
        lo.optimize();
        lo.bitor_assign_owned(T::from_values(typed[half..].to_vec()));
        out.push(lo);
        out
    }
    fn check<T: RoaringType>(vals: &[u64]) {
        let mut blobs = Vec::new();
        for mut b in histories::<T>(vals) {
            let blob = b.export_canonical().unwrap();
            assert_eq!(b.export_canonical().unwrap(), blob, "export twice");
            blobs.push(blob);
        }
        for (i, blob) in blobs.iter().enumerate() {
            assert_eq!(blob, &blobs[0], "history {i} differs");
        }
        let back = T::deserialize_from(Cursor::new(&blobs[0])).unwrap();
        assert_eq!(back.len(), vals.len() as u64);
    }
    let mut state = 0xA54F_F53A_5F1D_36F1u64;
    let mut shapes: Vec<Vec<u64>> = vec![
        tie(0xFFFF << 16, 3),
        [tie(0, 1), tie(0xFFFF << 16, 2)].concat(),
        [tie(0, 1), tie(1 << 16, 2), tie(2 << 16, 4)].concat(),
        (0..12u64).flat_map(|k| tie(k << 16, 1 + k % 5)).collect(),
    ];
    let mut beside_bitset = tie(0, 2);
    let mut dense: Vec<u64> = (0..6000)
        .map(|_| (1 << 16) | (xorshift(&mut state) % 65_536))
        .collect();
    dense.sort_unstable();
    dense.dedup();
    beside_bitset.extend(dense);
    beside_bitset.extend(tie(2 << 16, 7));
    shapes.push(beside_bitset);
    for s in &shapes {
        check::<RoaringBitmap>(s);
        check::<RoaringTreemap>(s);
        // R64: the same shape in the top sub-bitmap and across several.
        let top: Vec<u64> = s.iter().map(|&x| (((1u64 << 32) - 1) << 32) | x).collect();
        check::<RoaringTreemap>(&top);
        let spread: Vec<u64> = [0u64, 3, 1 << 31]
            .iter()
            .flat_map(|&h| s.iter().map(move |&x| (h << 32) | x))
            .collect();
        check::<RoaringTreemap>(&spread);
    }
}

/// Regression guard: the first export of a key whose containers are all
/// tied run containers re-encodes every one of them. Demoting them one at a
/// time shifted the container vector (R) or walked every sub-bitmap (R64)
/// per tie: 3.1 s and 0.67 s for the keys below. The linear demotion takes
/// ~15 ms and ~10 ms here; the bounds leave room for slow, instrumented CI
/// runs and still fail the quadratic version by a wide margin.
#[test]
fn export_canonical_tie_demotion_is_not_quadratic() {
    let mut r = RoaringBitmap::new();
    for k in 0..65_536u32 {
        r.insert_range(k << 16..(k << 16) + 3);
    }
    let t0 = std::time::Instant::now();
    r.export_canonical().unwrap();
    let dt = t0.elapsed();
    assert!(
        dt.as_millis() < 500,
        "R: 65,536 tied containers took {dt:?}"
    );

    let mut t = RoaringTreemap::new();
    for h in 0..16_384u64 {
        t.insert_range((h << 32) | 5..(h << 32) | 8);
    }
    let t0 = std::time::Instant::now();
    t.export_canonical().unwrap();
    let dt = t0.elapsed();
    assert!(
        dt.as_millis() < 250,
        "R64: 16,384 sub-bitmaps with one tie each took {dt:?}"
    );
}

/// The linear tie demotion on many ties mixed with containers it must leave
/// alone (strictly smaller runs, bitsets, arrays), with runs of adjacent
/// ties and gaps between them: the blob must equal that of the same set
/// built in bulk, and the set must not change.
#[test]
fn export_canonical_demotes_many_mixed_ties() {
    fn shape(k: u64) -> Vec<u64> {
        let lo = k << 16;
        match k % 7 {
            0..=2 => (lo + 5..lo + 8).collect(), // tie: 3 values, 1 run
            3 => (lo..lo + 5000).collect(),      // long run, not a tie
            4 => (0..5000).map(|i| lo + i * 7).collect(), // bitset
            5 => vec![lo + 3, lo + 9, lo + 400], // array
            _ => [lo + 1, lo + 2, lo + 3, lo + 7, lo + 8].to_vec(), // tie: 5 values, 2 runs
        }
    }
    fn check<T: RoaringType>(vals: &[u64]) {
        let v = |x: u64| T::Value::try_from(x).ok().unwrap();
        // Range-built: every run-shaped container starts run-encoded.
        let mut ranged = T::new();
        let mut i = 0;
        while i < vals.len() {
            let mut j = i;
            while j + 1 < vals.len() && vals[j + 1] == vals[j] + 1 {
                j += 1;
            }
            ranged.insert_range_exclusive(v(vals[i]), v(vals[j] + 1));
            i = j + 1;
        }
        let reference = T::from_values(vals.iter().map(|&x| v(x)).collect()).export_canonical();
        let before = ranged.clone();
        let blob = ranged.export_canonical().unwrap();
        assert_eq!(blob, reference.unwrap());
        assert_eq!(ranged, before, "demotion changed the set");
        assert_eq!(
            ranged.export_canonical().unwrap(),
            blob,
            "second export differs"
        );
    }
    let keys: Vec<u64> = (0..3000u64).filter(|k| k % 11 != 10).collect(); // gaps
    let vals: Vec<u64> = keys.iter().flat_map(|&k| shape(k)).collect();
    check::<RoaringBitmap>(&vals);
    // R64: the same containers spread over sub-bitmaps around a 2^32 border.
    let spread: Vec<u64> = vals
        .iter()
        .map(|&x| (((x >> 16) % 5 + (1 << 31)) << 32) | (x & 0xFFFF_FFFF))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    check::<RoaringTreemap>(&spread);
}

/// Unions pick between roaring-rs's in-place `|=` (no copies, but one vector
/// insert per missing container) and a one-pass merge. Both must give the
/// operator's result: few or many missing containers, all present, big and
/// small sides, owned and borrowed, both widths, and through the BITOP and
/// IMPORT paths that use them.
#[test]
fn union_strategies_match_the_operator() {
    use crate::bitmap32::union_in_place_is_cheap;
    let even: RoaringBitmap = (0..3_000u32).map(|k| ((2 * k) << 16) | 7).collect();
    let odd: RoaringBitmap = (0..3_000u32).map(|k| ((2 * k + 1) << 16) | 9).collect();
    let few: RoaringBitmap = (0..5u32).map(|k| ((2 * k + 1) << 16) | 3).collect();
    let present: RoaringBitmap = (0..200u32).map(|k| ((2 * k) << 16) | 11).collect();
    let nine_absent: RoaringBitmap = (0..9u32)
        .map(|k| ((2 * k + 1) << 16) | 3)
        .chain([5 << 16])
        .collect();
    let forty_absent: RoaringBitmap = (0..40u32)
        .map(|k| ((2 * k + 1) << 16) | 3)
        .chain((0..20u32).map(|k| (2 * k) << 16))
        .collect();
    // A small left side taking a big disjoint right side: the merge.
    let small: RoaringBitmap = (0..15u32).map(|k| (k * 400) << 16).collect();
    assert!(union_in_place_is_cheap(&even, &few));
    assert!(union_in_place_is_cheap(&even, &present)); // 200 containers, none missing
    assert!(union_in_place_is_cheap(&even, &nine_absent)); // few to insert
    assert!(!union_in_place_is_cheap(&even, &forty_absent)); // 40 shifts of 3,000
    assert!(!union_in_place_is_cheap(&even, &odd)); // 3,000 interleaved
    assert!(!union_in_place_is_cheap(&small, &odd));
    for other in [
        &odd,
        &few,
        &present,
        &nine_absent,
        &forty_absent,
        &small,
        &RoaringBitmap::new(),
    ] {
        let expected = &even | other;
        let mut a = even.clone();
        RoaringType::bitor_assign(&mut a, other);
        assert_eq!(a, expected);
        let mut b = even.clone();
        RoaringType::bitor_assign_owned(&mut b, other.clone());
        assert_eq!(b, expected);
        let mut c = other.clone();
        RoaringType::bitor_assign_owned(&mut c, even.clone());
        assert_eq!(c, expected);
        assert_eq!(
            crate::commands_bitop::op_or(&[&even, other, &few]),
            &expected | &few
        );
    }
    // R64: sub-bitmaps only on one side, shared ones with few and many
    // missing containers.
    let lift = |b: &RoaringBitmap, his: &[u64]| -> RoaringTreemap {
        his.iter()
            .flat_map(|&h| b.iter().map(move |v| (h << 32) | u64::from(v)))
            .collect()
    };
    let te = lift(&even, &[0, 2, 9]);
    for other in [
        lift(&odd, &[2, 5]),
        lift(&few, &[0, 7]),
        lift(&present, &[9]),
        lift(&nine_absent, &[0]),
    ] {
        let expected = &te | &other;
        let mut a = te.clone();
        RoaringType::bitor_assign(&mut a, &other);
        assert_eq!(a, expected);
        let mut b = te.clone();
        RoaringType::bitor_assign_owned(&mut b, other.clone());
        assert_eq!(b, expected);
        assert_eq!(RoaringType::union(&other, &te), expected);
    }
}

/// The all-tied keys above take the demotion's shortcut (the tied arrays
/// are the whole set). Ties interleaved with containers that stay as they
/// are go through the general merge path, which must be linear too: a
/// per-container remove and re-insert costs seconds at this size.
#[test]
fn export_canonical_mixed_ties_is_linear() {
    let mut r = RoaringBitmap::new();
    for k in 0..65_536u32 {
        let len = if k % 2 == 0 { 3 } else { 20 }; // a tie, then a strictly smaller run
        r.insert_range(k << 16..(k << 16) + len);
    }
    let reference =
        <RoaringBitmap as RoaringType>::from_values(r.iter().collect()).export_canonical();
    let t0 = std::time::Instant::now();
    let blob = r.export_canonical().unwrap();
    let dt = t0.elapsed();
    assert!(
        dt.as_millis() < 250,
        "R: 32,768 ties among 65,536 containers took {dt:?}"
    );
    assert_eq!(blob, reference.unwrap());

    let mut t = RoaringTreemap::new();
    for h in 0..1_024u64 {
        for k in 0..64u64 {
            let lo = (h << 32) | (k << 16);
            t.insert_range(lo..lo + if k % 2 == 1 { 3 } else { 20 });
        }
    }
    let reference =
        <RoaringTreemap as RoaringType>::from_values(t.iter().collect()).export_canonical();
    let t0 = std::time::Instant::now();
    let blob = t.export_canonical().unwrap();
    let dt = t0.elapsed();
    assert!(
        dt.as_millis() < 250,
        "R64: 32,768 ties over 1,024 sub-bitmaps took {dt:?}"
    );
    assert_eq!(blob, reference.unwrap());
}

/// IMPORT's merge and BITOP's later sources add containers the left side
/// lacks. roaring-rs's `|=` inserts each one into the container vector, so
/// interleaved keys made it quadratic (~190 ms per 32,768 containers in the
/// server); the merge must stay linear. Eight interleaved sub-bitmap pairs
/// would take over a second the quadratic way.
#[test]
fn union_of_interleaved_containers_is_linear() {
    let even: RoaringBitmap = (0..32_768u32).map(|k| ((2 * k) << 16) | 7).collect();
    let odd: RoaringBitmap = (0..32_768u32).map(|k| ((2 * k + 1) << 16) | 9).collect();
    let t0 = std::time::Instant::now();
    let mut a = even.clone();
    RoaringType::bitor_assign_owned(&mut a, odd.clone());
    let mut b = odd.clone();
    RoaringType::bitor_assign(&mut b, &even);
    let merged = crate::commands_bitop::op_or(&[&even, &RoaringBitmap::new(), &odd]);
    let dt = t0.elapsed();
    assert!(
        dt.as_millis() < 150,
        "R: three interleaved unions took {dt:?}"
    );
    assert_eq!(a.len(), 65_536);
    assert_eq!(a, b);
    assert_eq!(a, merged);

    let lift = |bm: &RoaringBitmap| -> RoaringTreemap {
        (0..8u64)
            .flat_map(|h| bm.iter().map(move |v| (h << 32) | u64::from(v)))
            .collect()
    };
    let (te, to) = (lift(&even), lift(&odd));
    let t0 = std::time::Instant::now();
    let mut c = te.clone();
    RoaringType::bitor_assign_owned(&mut c, to.clone());
    let dt = t0.elapsed();
    assert!(
        dt.as_millis() < 300,
        "R64: 8 interleaved sub-bitmap unions took {dt:?}"
    );
    assert_eq!(c.len(), 8 * 65_536);
}

/// Regression guard: R64.SETRANGE's no-op check, contains_range_exclusive,
/// once summed the cardinality of the whole treemap (RoaringTreemap::len
/// walks every container) and scanned sub-bitmaps from the first, so a
/// 5-value SETRANGE on a key of a million containers cost ~0.7 ms per call
/// (1.1.1: ~0.3 us). It now visits only the range's sub-bitmaps.
#[test]
fn r64_contains_range_cost_does_not_scale_with_the_key() {
    let t: RoaringTreemap = (0..1_000_000u64).map(|i| i << 16).collect();
    let t0 = std::time::Instant::now();
    let mut hits = 0;
    for i in 0..1_000u64 {
        let lo = (i * 997) << 16;
        hits += usize::from(RoaringType::contains_range_exclusive(&t, lo + 20, lo + 25));
    }
    let dt = t0.elapsed();
    assert_eq!(hits, 0);
    assert!(
        dt.as_millis() < 50,
        "1,000 checks on 1M containers took {dt:?}"
    );
}

/// The union strategy switches at three edges: 16 containers on the right
/// (always in place), the shift budget (in place while inserting the
/// missing containers stays cheap), and the 1/8 size ratio below which the
/// missing containers are counted. Around each edge, with every right-side
/// container missing, all present, or a mix, owned and borrowed, both
/// widths: the result must equal the operator's, and no union of this size
/// (at most 65,536 containers a side) may take more than a few
/// milliseconds, which a per-container insert into a long vector would.
#[test]
fn union_rule_edges_are_correct_and_cheap() {
    fn keys(range: std::ops::Range<u32>, step: u32, offset: u32) -> RoaringBitmap {
        range
            .step_by(step as usize)
            .map(|k| (k << 16) | offset)
            .collect()
    }
    let mut worst = std::time::Duration::ZERO;
    for n_bm in [64u32, 1_000, 8_000, 32_768] {
        let left = keys(0..2 * n_bm, 2, 7); // even container keys
        for n_other in [
            15u32,
            16,
            17,
            n_bm / 8 - 1,
            n_bm / 8,
            n_bm / 8 + 1,
            n_bm / 2,
            n_bm,
        ] {
            if n_other == 0 {
                continue;
            }
            for kind in 0..3 {
                let right: RoaringBitmap = match kind {
                    0 => keys(0..2 * n_other, 2, 9)
                        .iter()
                        .map(|v| v + (1 << 16))
                        .collect(), // all missing
                    1 => keys(0..2 * n_other, 2, 9), // all present
                    _ => keys(0..n_other, 1, 9),     // half and half
                };
                let expected = &left | &right;
                let t0 = std::time::Instant::now();
                let mut a = left.clone();
                RoaringType::bitor_assign(&mut a, &right);
                let mut b = left.clone();
                RoaringType::bitor_assign_owned(&mut b, right.clone());
                worst = worst.max(t0.elapsed() / 2);
                assert_eq!(a, expected, "n_bm={n_bm} n_other={n_other} kind={kind}");
                assert_eq!(
                    b, expected,
                    "owned n_bm={n_bm} n_other={n_other} kind={kind}"
                );
                // R64: the same pair in a shared sub-bitmap, next to
                // sub-bitmaps only one side has.
                let lift = |bm: &RoaringBitmap, his: &[u64]| -> RoaringTreemap {
                    his.iter()
                        .flat_map(|&h| bm.iter().map(move |v| (h << 32) | u64::from(v)))
                        .collect()
                };
                let (tl, tr) = (lift(&left, &[1, 3]), lift(&right, &[3, 5]));
                let mut c = tl.clone();
                RoaringType::bitor_assign_owned(&mut c, tr.clone());
                assert_eq!(
                    c,
                    &tl | &tr,
                    "R64 n_bm={n_bm} n_other={n_other} kind={kind}"
                );
            }
        }
    }
    assert!(worst.as_millis() < 40, "slowest union took {worst:?}");
}

/// EXPORT's tie scan reads run counts through the blob's offset table and
/// checks only the last container's end. On layouts of up to 20,000
/// containers mixing ties (1 to 2,047 runs, up to 4,095 values), smaller
/// runs, arrays, bitsets and full containers, at random keys and in both
/// widths, the export must equal the bulk-built set's and decode to the
/// same values.
#[test]
fn export_canonical_on_large_mixed_layouts() {
    let mut state = 0xFEED_FACE_CAFE_BEEFu64;
    for round in 0..4 {
        let n = [5usize, 64, 1_000, 20_000][round];
        let mut keys: Vec<u32> = (0..65_536u32).collect();
        for i in (1..keys.len()).rev() {
            keys.swap(i, (xorshift(&mut state) % (i as u64 + 1)) as usize);
        }
        let mut chosen: Vec<u32> = keys[..n].to_vec();
        chosen.sort_unstable();
        // Ranges [start, end) and single values, applied to both widths.
        let mut ranges: Vec<(u32, u32)> = Vec::new();
        let mut singles: Vec<u32> = Vec::new();
        for &k in &chosen {
            let lo = k << 16;
            match xorshift(&mut state) % 6 {
                0 => {
                    let r = 1 + (xorshift(&mut state) % 2047) as u32;
                    let mut pos = lo + 1;
                    for i in 0..r {
                        let len = if i == 0 { 3 } else { 2 };
                        ranges.push((pos, pos + len));
                        pos += len + 1;
                    }
                }
                1 => ranges.push((lo, lo + 2 + (xorshift(&mut state) % 3_000) as u32)),
                2 => singles.extend(
                    (0..1 + xorshift(&mut state) % 300)
                        .map(|_| lo | (xorshift(&mut state) % 65_536) as u32),
                ),
                3 => {
                    singles.extend((0..5_000).map(|_| lo | (xorshift(&mut state) % 65_536) as u32))
                }
                4 if n <= 1_000 => ranges.push((lo, lo | 0xFFFF)),
                _ => {
                    ranges.push((lo + 10, lo + 13));
                    ranges.push((lo + 20, lo + 22)); // 5 values, 2 runs: a tie
                }
            }
        }
        let mut b = RoaringBitmap::new();
        let mut t = RoaringTreemap::new();
        let hi = |v: u32| (u64::from(v >> 16) % 3) << 32;
        for &(a, z) in &ranges {
            b.insert_range(a..z);
            t.insert_range(hi(a) | u64::from(a)..hi(a) | u64::from(z));
        }
        for &v in &singles {
            b.insert(v);
            t.insert(hi(v) | u64::from(v));
        }
        let vals: Vec<u32> = b.iter().collect();
        let want = <RoaringBitmap as RoaringType>::from_values(vals.clone())
            .export_canonical()
            .unwrap();
        let blob = b.export_canonical().unwrap();
        assert_eq!(blob, want, "R round {round}, {n} containers");
        let back = <RoaringBitmap as RoaringType>::deserialize_from(Cursor::new(&blob)).unwrap();
        assert!(back.iter().eq(vals.iter().copied()));
        let want64 = <RoaringTreemap as RoaringType>::from_values(t.iter().collect())
            .export_canonical()
            .unwrap();
        assert_eq!(t.export_canonical().unwrap(), want64, "R64 round {round}");
    }
}

/// CONTAINS EQ's set equality against roaring-rs's `==`, across arrays,
/// bitsets, runs and mixed encodings of the same set (the fast path
/// compares bitsets a word at a time), and ALL_STRICT's length test.
#[test]
fn set_eq_matches_the_operator_across_encodings() {
    let shapes: Vec<RoaringBitmap> = vec![
        RoaringBitmap::new(),
        (0..100u32).map(|v| v * 7).collect(),
        (0..20_000u32).map(|v| v * 3).collect(), // bitsets
        (0..200_000u32).filter(|v| v % 1000 < 900).collect(), // runs once optimized
        (0..70_000u32).chain(1 << 20..(1 << 20) + 50).collect(),
    ];
    for a in &shapes {
        for b in &shapes {
            let mut b_opt = b.clone();
            b_opt.optimize();
            let mut b_plain = b.clone();
            b_plain.remove_run_compression();
            for other in [b, &b_opt, &b_plain] {
                assert_eq!(RoaringType::set_eq(a, other), a == other);
                let ta: RoaringTreemap = a.iter().map(|v| u64::from(v) << 3).collect();
                let tb: RoaringTreemap = other.iter().map(|v| u64::from(v) << 3).collect();
                assert_eq!(RoaringType::set_eq(&ta, &tb), ta == tb);
            }
            // One value apart: never equal, ALL_STRICT by length.
            let mut c = a.clone();
            c.insert(123_456_789);
            assert!(!RoaringType::set_eq(a, &c));
            assert_eq!(
                a.is_subset(&c) && a.len() != c.len(),
                a.is_subset(&c) && *a != c
            );
        }
    }
}

/// The exact-size builder (SETINTARRAY, SETBITARRAY) against roaring-rs's
/// `from_sorted_iter`: the same set and the same container encodings
/// (STAT's breakdown), at the array/bitset boundary and the ends of the
/// value range, and no growth slack left to compact.
#[test]
fn exact_builder_matches_from_sorted_iter() {
    use crate::bitmap32::{containers_heap_size, from_sorted_exact, worth_compacting};
    let mut x: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let mut cases: Vec<Vec<u32>> = vec![
        vec![],
        vec![0],
        vec![u32::MAX],
        (0..4096).collect(),
        (0..4097).collect(),
        (65_536 - 3..65_536 + 3).collect(),
        (u32::MAX - 5000..=u32::MAX).collect(),
    ];
    for _ in 0..40 {
        let n = (next() % 30_000) as usize;
        let span = 1 + next() % (1 << (10 + next() % 22));
        let mut v: Vec<u32> = (0..n).map(|_| (next() % span) as u32).collect();
        v.sort_unstable();
        v.dedup();
        cases.push(v);
    }
    for vals in &cases {
        let exact = from_sorted_exact(vals);
        let grown = RoaringBitmap::from_sorted_iter(vals.iter().copied()).unwrap();
        assert_eq!(exact, grown);
        let (se, sg) = (exact.statistics(), grown.statistics());
        assert_eq!(
            (
                se.n_array_containers,
                se.n_bitset_containers,
                se.n_run_containers
            ),
            (
                sg.n_array_containers,
                sg.n_bitset_containers,
                sg.n_run_containers
            )
        );
        let (heap, slack, n) = containers_heap_size(&exact);
        assert!(
            n == 0 || !worth_compacting(heap, slack, n),
            "nothing for trim to do: heap {heap} slack {slack} containers {n}"
        );
        let wide: Vec<u64> = vals
            .iter()
            .map(|&v| (u64::from(v) << 7) ^ 0x5_0000_0000)
            .collect();
        let mut sorted = wide.clone();
        sorted.sort_unstable();
        assert_eq!(
            RoaringTreemap::from_values(wide),
            RoaringTreemap::from_sorted_iter(sorted).unwrap()
        );
    }
}

/// NOT's direct complement against the full-range XOR it replaced, for
/// every kind of source chunk (absent, sparse, clustered, dense, full, runs
/// crossing chunk edges) and every kind of `last` (chunk edges, u32::MAX,
/// the source maximum), on both widths.
#[test]
fn complement_matches_the_range_xor() {
    fn reference32(src: &RoaringBitmap, last: u32) -> RoaringBitmap {
        let mut r = RoaringBitmap::new();
        r.insert_range(0..=last);
        r ^= src;
        r
    }
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let mut sources: Vec<RoaringBitmap> = vec![
        RoaringBitmap::new(),
        [0u32].into_iter().collect(),
        [65_535u32, 65_536].into_iter().collect(),
        (0..65_536u32).collect(),                         // one full chunk
        (65_530..131_080u32).collect(),                   // a run across two edges
        (0..200_000u32).filter(|v| v % 3 != 0).collect(), // dense, many gaps
        [u32::MAX].into_iter().collect(),
        (u32::MAX - 70_000..=u32::MAX).collect(),
    ];
    for _ in 0..30 {
        let span = 1 + next() % (1u64 << (12 + next() % 21));
        let n = (next() % 40_000) as usize;
        let mut b: RoaringBitmap = (0..n).map(|_| (next() % span) as u32).collect();
        if next() % 3 == 0 {
            let s = (next() % span) as u32;
            b.insert_range(s..s.saturating_add((next() % 100_000) as u32));
        }
        if next() % 2 == 0 {
            b.optimize();
        }
        sources.push(b);
    }
    for src in &sources {
        let max = src.max().unwrap_or(0);
        let mut lasts = vec![max, max.saturating_add(1), max | 0xFFFF, u32::MAX];
        lasts.push(max.saturating_add((next() % 300_000) as u32));
        if max < 65_535 {
            lasts.extend([65_535, 65_536, 131_071]);
        }
        for last in lasts {
            if last < max {
                continue;
            }
            let fast = crate::bitmap32::complement_within(src, last);
            assert_eq!(fast, reference32(src, last), "last {last}");
            assert_eq!(RoaringType::len(&fast), u64::from(last) + 1 - src.len());
        }
        // Above `last`: the general path, values kept.
        if max > 10 {
            assert_eq!(RoaringType::flip_inclusive(src, 10), reference32(src, 10));
        }
        // 64-bit: the same source in two blocks, `last` in a third.
        let t: RoaringTreemap = src
            .iter()
            .flat_map(|v| [u64::from(v), (1u64 << 32) | u64::from(v)])
            .collect();
        for last in [(1u64 << 32) | u64::from(max), (2u64 << 32) + 5] {
            let mut r = RoaringTreemap::new();
            r.insert_range(0..=last);
            r ^= &t;
            assert_eq!(RoaringType::flip_inclusive(&t, last), r, "R64 last {last}");
        }
    }
}
