//! valkey-roaring: Core trait abstracting over RoaringBitmap (u32) and RoaringTreemap (u64).

use crate::parse;
use std::fmt;
use std::io;

/// Decode one portable blob that must span `bytes` exactly: trailing bytes
/// mean it is not a single valid bitmap (R.IMPORT and RDB loading).
pub(crate) fn decode_exact<T: RoaringType>(bytes: &[u8]) -> io::Result<T> {
    let mut cursor = io::Cursor::new(bytes);
    let value = T::deserialize_from(&mut cursor)?;
    if cursor.position() != bytes.len() as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "trailing bytes after the blob",
        ));
    }
    Ok(value)
}

/// R.STAT's fields, in CRoaring's units:
/// array container bytes are 2 per value, bitset container bytes 8192 per
/// container, run container bytes each container's serialized size (2 plus
/// 4 per run). An empty bitmap reports max 0 and min the width's maximum.
pub struct StatFields {
    pub kind: &'static str,
    pub cardinality: u64,
    pub containers: u64,
    pub max: u64,
    pub min: u64,
    pub arrays: u64,
    pub array_values: u64,
    pub array_bytes: u64,
    pub bitsets: u64,
    pub bitset_values: u64,
    pub bitset_bytes: u64,
    pub runs: u64,
    pub run_values: u64,
    pub run_bytes: u64,
}

impl StatFields {
    pub fn text(&self) -> String {
        format!(
            "type: {}\n\
             cardinality: {}\n\
             number of containers: {}\n\
             max value: {}\n\
             min value: {}\n\
             number of array containers: {}\n\
             \tarray container values: {}\n\
             \tarray container bytes: {}\n\
             bitset  containers: {}\n\
             \tbitset  container values: {}\n\
             \tbitset  container bytes: {}\n\
             run containers: {}\n\
             \trun container values: {}\n\
             \trun container bytes: {}\n",
            self.kind,
            self.cardinality,
            self.containers,
            self.max,
            self.min,
            self.arrays,
            self.array_values,
            self.array_bytes,
            self.bitsets,
            self.bitset_values,
            self.bitset_bytes,
            self.runs,
            self.run_values,
            self.run_bytes,
        )
    }

    pub fn json(&self) -> String {
        format!(
            "{{\"type\":\"{}\",\
             \"cardinality\":\"{}\",\
             \"number_of_containers\":\"{}\",\
             \"max_value\":\"{}\",\
             \"min_value\":\"{}\",\
             \"array_container\":{{\"number_of_containers\":\"{}\",\"container_cardinality\":\"{}\",\"container_allocated_bytes\":\"{}\"}},\
             \"bitset_container\":{{\"number_of_containers\":\"{}\",\"container_cardinality\":\"{}\",\"container_allocated_bytes\":\"{}\"}},\
             \"run_container\":{{\"number_of_containers\":\"{}\",\"container_cardinality\":\"{}\",\"container_allocated_bytes\":\"{}\"}}}}",
            self.kind,
            self.cardinality,
            self.containers,
            self.max,
            self.min,
            self.arrays,
            self.array_values,
            self.array_bytes,
            self.bitsets,
            self.bitset_values,
            self.bitset_bytes,
            self.runs,
            self.run_values,
            self.run_bytes,
        )
    }
}

/// Trait abstracting the common interface of RoaringBitmap and RoaringTreemap.
/// Each command handler is generic over this trait so it's written once, registered twice.
pub trait RoaringType: Send + Sync + Clone + PartialEq + fmt::Debug + 'static {
    type Value: Copy + Ord + fmt::Display + TryFrom<u64> + 'static;

    /// Largest value of the width.
    const MAX_VALUE: Self::Value;
    /// Upstream's description of a valid value argument, for error replies
    /// ("ERR invalid <name>: <description>").
    const VALUE_DESCRIPTION: &'static str;
    /// Upstream's SETRANGE reply for end < start (its 64-bit text lacks "be").
    const ERR_END_BEFORE_START: &'static str;
    /// Whether GETBIT validates the offset before answering 0 for a missing
    /// key: upstream's R64.GETBIT does, its R.GETBIT does not.
    const GETBIT_PARSES_BEFORE_KEY_CHECK: bool;

    /// Parse a value argument with upstream's grammar for the width.
    fn parse_value_arg(bytes: &[u8]) -> Option<Self::Value>;
    /// valkey-roaring 1.1.1's grammar (an optional '+', digits, leading
    /// zeros allowed, within the width), for commands replayed from its AOF
    /// or replication stream; see commands::from_aof_or_primary.
    fn parse_value_legacy(bytes: &[u8]) -> Option<Self::Value> {
        Self::Value::try_from(parse::parse_u64_bytes(bytes)?).ok()
    }

    /// Convert a value to i64 for Valkey replies. Values > i64::MAX become i64::MAX.
    fn value_to_i64(v: Self::Value) -> i64;
    fn value_to_u64(v: Self::Value) -> u64;

    // -- Construction --
    fn new() -> Self;
    fn full() -> Self;
    /// Build from values in any order, duplicates allowed. Takes the vector
    /// to sort it in place: bulk-appending sorted values is several times
    /// faster than inserting them one by one.
    fn from_values(vals: Vec<Self::Value>) -> Self;

    // -- Element ops --
    fn insert(&mut self, v: Self::Value) -> bool;
    fn remove(&mut self, v: Self::Value) -> bool;
    fn contains(&self, v: Self::Value) -> bool;
    fn clear(&mut self);

    // -- Bulk ops --
    /// Insert many values, returning the count of bits that were not already set.
    fn insert_many(&mut self, vals: &[Self::Value]) -> usize;
    /// Remove many values, returning the count of bits that were actually set.
    fn remove_many(&mut self, vals: &[Self::Value]) -> usize;
    /// True when every value in [start, end) is present (vacuously for an
    /// empty range).
    fn contains_range_exclusive(&self, start: Self::Value, end: Self::Value) -> bool;

    // -- Cardinality --
    fn len(&self) -> u64;
    /// Number of roaring containers, i.e. separate allocations to free.
    fn container_count(&self) -> usize;
    fn min_val(&self) -> Option<Self::Value>;
    fn max_val(&self) -> Option<Self::Value>;

    // -- Set operations (in-place, other borrowed: nothing is cloned) --
    fn bitor_assign(&mut self, other: &Self);
    fn bitand_assign(&mut self, other: &Self);
    fn bitxor_assign(&mut self, other: &Self);
    fn sub_assign(&mut self, other: &Self);
    /// Union consuming `other`: its containers move instead of being copied.
    fn bitor_assign_owned(&mut self, other: Self);

    // -- Set operations (new set from two borrowed ones) --
    fn union(&self, other: &Self) -> Self;
    fn intersection(&self, other: &Self) -> Self;
    fn symmetric_difference(&self, other: &Self) -> Self;
    fn difference(&self, other: &Self) -> Self;

    // -- Comparisons --
    fn is_disjoint(&self, other: &Self) -> bool;
    fn is_subset(&self, other: &Self) -> bool;
    /// Set equality (CONTAINS EQ). roaring-rs's `==` compares two bitset
    /// containers value by value through their bit iterators (19-116x
    /// slower than upstream on dense keys); see the implementations.
    fn set_eq(&self, other: &Self) -> bool;

    // -- Cardinality without materialization --
    fn intersection_len(&self, other: &Self) -> u64;

    // -- Positional --
    /// Returns the nth element (0-indexed).
    fn select(&self, n: u64) -> Option<Self::Value>;
    /// Returns the nth absent element (1-indexed).
    fn nth_absent(&self, n: u64) -> Option<Self::Value>;

    // -- NOT/Flip --
    /// Return complement of bitmap in [0, last] (inclusive). Bits above `last`
    /// are preserved as-is, matching CRoaring's flip semantics.
    fn flip_inclusive(&self, last: Self::Value) -> Self;

    // -- Serialization --
    fn serialize_into<W: io::Write>(&self, writer: W) -> io::Result<()>;
    fn deserialize_from<R: io::Read>(reader: R) -> io::Result<Self>;
    /// valkey-roaring 1.1.1's IMPORT decoding, for commands replayed from its
    /// AOF or replication stream: bytes after a complete blob are ignored,
    /// and 64-bit high words may repeat (the last entry wins, dropping the
    /// earlier ones' values, as 1.1.1 did) or decrease. Empty sub-bitmaps
    /// are dropped, which leaves the set unchanged.
    fn deserialize_legacy(bytes: &[u8]) -> io::Result<Self>;
    fn serialized_size(&self) -> usize;
    /// R.EXPORT's blob: optimize in place, then serialize so that one logical
    /// set always yields the same bytes, whatever built it (canonical.rs).
    fn export_canonical(&mut self) -> io::Result<Vec<u8>>;

    // -- Optimization / memory --
    fn optimize(&mut self) -> bool;
    /// Re-allocate every container at its exact size. Bulk construction and
    /// set operations leave growth slack behind (roaring-rs never shrinks a
    /// container's vector); a clone allocates each one at its length.
    fn compact(&mut self) {
        *self = self.clone();
    }
    /// Compact only when containers likely carry enough growth slack to be
    /// worth the copy. Set operations size result containers for the worst
    /// case (an AND keeps the capacity of its larger input), which can leave
    /// a small result holding most of the memory of its inputs.
    fn trim(&mut self);
    /// Estimated heap bytes owned by the value, for MEMORY USAGE.
    fn heap_size(&self) -> usize;

    // -- Range operations --
    /// Insert every value in [start, end) — end-exclusive, like CRoaring's
    /// add_range (R.SETRANGE's semantics).
    fn insert_range_exclusive(&mut self, start: Self::Value, end: Self::Value) -> u64;

    // -- Iterator --
    /// The value iterator: roaring-rs's own, so the reply loops over it
    /// monomorphize (a boxed iterator cost a dynamic call per value).
    type Values<'a>: Iterator<Item = Self::Value>
    where
        Self: 'a;
    fn iter_values(&self) -> Self::Values<'_>;
    /// Ascending iterator over the values >= `start`.
    fn iter_from(&self, start: Self::Value) -> Self::Values<'_>;

    // -- Bit array --
    fn from_bit_array(bits: &[u8]) -> Self;
    fn to_bit_array(&self) -> Vec<u8>;

    // -- Statistics --
    /// R.STAT's fields (see StatFields).
    fn stat_fields(&self) -> StatFields;
}
