//! Fuzzes R-vs-R64 parity: the same operation sequence applied to both
//! bitmap widths must produce identical results for u32-range values.
#![no_main]

use libfuzzer_sys::fuzz_target;
use roaring::{RoaringBitmap, RoaringTreemap};
use valkey_roaring::fuzzing::RoaringType;

fuzz_target!(|data: &[u8]| {
    let mut b32 = RoaringBitmap::new();
    let mut b64 = RoaringTreemap::new();

    for c in data.chunks_exact(3) {
        // Small u16 universe so operations collide often.
        let v = u16::from_le_bytes([c[1], c[2]]) as u32;
        match c[0] % 6 {
            0 => assert_eq!(b32.insert(v), b64.insert(v as u64)),
            1 => assert_eq!(b32.remove(v), b64.remove(v as u64)),
            2 => assert_eq!(b32.contains(v), b64.contains(v as u64)),
            3 => assert_eq!(
                RoaringType::nth_absent(&b32, 1).map(u64::from),
                RoaringType::nth_absent(&b64, 1)
            ),
            4 => assert_eq!(
                RoaringType::select(&b32, (v % 64) as u64).map(u64::from),
                RoaringType::select(&b64, (v % 64) as u64)
            ),
            _ => {
                let f32v = RoaringType::flip_inclusive(&b32, v);
                let f64v = RoaringType::flip_inclusive(&b64, v as u64);
                b32 = f32v;
                b64 = f64v;
            }
        }
    }

    assert_eq!(b32.len(), b64.len());
    assert_eq!(b32.min().map(u64::from), b64.min());
    assert_eq!(b32.max().map(u64::from), b64.max());
    assert!(b32.iter().map(u64::from).eq(b64.iter()));
});
