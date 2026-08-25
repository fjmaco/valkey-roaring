//! Fuzzes the R.IMPORT attack surface: arbitrary untrusted bytes fed into
//! deserialize_from for both bitmap widths. A panic here would abort the
//! whole Valkey server in production. Successful parses must round-trip.
#![no_main]

use libfuzzer_sys::fuzz_target;
use roaring::{RoaringBitmap, RoaringTreemap};
use std::io::Cursor;

fuzz_target!(|data: &[u8]| {
    if let Ok(bm) = RoaringBitmap::deserialize_from(Cursor::new(data)) {
        let mut out = Vec::new();
        bm.serialize_into(&mut out).unwrap();
        let back = RoaringBitmap::deserialize_from(Cursor::new(&out[..])).unwrap();
        assert_eq!(bm, back);
    }
    if let Ok(tm) = RoaringTreemap::deserialize_from(Cursor::new(data)) {
        let mut out = Vec::new();
        tm.serialize_into(&mut out).unwrap();
        let back = RoaringTreemap::deserialize_from(Cursor::new(&out[..])).unwrap();
        assert_eq!(tm, back);
    }
});
