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
