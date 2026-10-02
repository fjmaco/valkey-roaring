//! Fuzzes R.EXPORT's canonical blob. Arbitrary bytes are deserialized the
//! way R.IMPORT does (through the module's trait, which also drops empty
//! R64 sub-bitmaps); whatever encodings that leaves in memory, the export
//! must decode back to the same set, repeat byte-identically, and equal the
//! export of the same values rebuilt in bulk (one set, one blob).
#![no_main]

use libfuzzer_sys::fuzz_target;
use roaring::{RoaringBitmap, RoaringTreemap};
use std::io::Cursor;
use valkey_roaring::fuzzing::RoaringType;

fn check<T: RoaringType>(data: &[u8]) {
    let Ok(mut imported) = T::deserialize_from(Cursor::new(data)) else {
        return;
    };
    let values: Vec<T::Value> = imported.iter_values().collect();
    let blob = imported.export_canonical().unwrap();
    assert_eq!(imported.export_canonical().unwrap(), blob, "export twice");
    let back = T::deserialize_from(Cursor::new(&blob)).unwrap();
    assert!(back.iter_values().eq(values.iter().copied()), "decode");
    let mut rebuilt = T::from_values(values);
    assert_eq!(rebuilt.export_canonical().unwrap(), blob, "canonical");
}

fuzz_target!(|data: &[u8]| {
    check::<RoaringBitmap>(data);
    check::<RoaringTreemap>(data);
});
