//! valkey-roaring: Core trait abstracting over RoaringBitmap (u32) and RoaringTreemap (u64).

use std::fmt;
use std::io;

/// Trait abstracting the common interface of RoaringBitmap and RoaringTreemap.
/// Each command handler is generic over this trait so it's written once, registered twice.
pub trait RoaringType: Send + Sync + Clone + PartialEq + fmt::Debug + 'static {
    type Value: Copy + Ord + fmt::Display + TryFrom<u64> + 'static;

    /// Convert a value to i64 for Valkey replies. Values > i64::MAX become i64::MAX.
    fn value_to_i64(v: Self::Value) -> i64;

    // -- Construction --
    fn new() -> Self;
    fn full() -> Self;
    fn from_values(vals: &[Self::Value]) -> Self;

    // -- Element ops --
    fn insert(&mut self, v: Self::Value) -> bool;
    fn remove(&mut self, v: Self::Value) -> bool;
    fn contains(&self, v: Self::Value) -> bool;
    fn clear(&mut self);

    // -- Bulk ops --
    fn insert_many(&mut self, vals: &[Self::Value]);
    fn remove_many(&mut self, vals: &[Self::Value]);
    fn contains_many(&self, vals: &[Self::Value]) -> Vec<bool>;
    /// Remove many values, returning the count of bits that were actually set.
    fn remove_many_counted(&mut self, vals: &[Self::Value]) -> usize;

    // -- Cardinality --
    fn len(&self) -> u64;
    fn min_val(&self) -> Option<Self::Value>;
    fn max_val(&self) -> Option<Self::Value>;

    // -- Set operations (in-place) --
    fn bitor_assign(&mut self, other: &Self);
    fn bitand_assign(&mut self, other: &Self);
    fn bitxor_assign(&mut self, other: &Self);
    fn sub_assign(&mut self, other: &Self);

    // -- Set operations (owned) --
    fn sub_owned(self, other: Self) -> Self;

    // -- Comparisons --
    fn is_disjoint(&self, other: &Self) -> bool;
    fn is_subset(&self, other: &Self) -> bool;

    // -- Cardinality without materialization --
    fn intersection_len(&self, other: &Self) -> u64;
    fn union_len(&self, other: &Self) -> u64;

    // -- Positional --
    /// Returns the nth element (0-indexed).
    fn select(&self, n: u64) -> Option<Self::Value>;
    /// Returns the nth absent element (1-indexed, matching C module).
    fn nth_absent(&self, n: u64) -> Option<Self::Value>;

    // -- NOT/Flip --
    /// Return complement of bitmap in [0, last] (inclusive). Bits above `last`
    /// are preserved as-is, matching CRoaring's flip semantics.
    fn flip_inclusive(&self, last: Self::Value) -> Self;

    // -- Serialization --
    fn serialize_into<W: io::Write>(&self, writer: W) -> io::Result<()>;
    fn deserialize_from<R: io::Read>(reader: R) -> io::Result<Self>;
    fn serialized_size(&self) -> usize;

    // -- Optimization --
    fn optimize(&mut self) -> bool;

    // -- Range operations --
    /// Insert every value in [start, end) — end-exclusive, like CRoaring's
    /// add_range (redis-roaring's SETRANGE semantics).
    fn insert_range_exclusive(&mut self, start: Self::Value, end: Self::Value) -> u64;

    // -- Iterator --
    fn iter_values(&self) -> Box<dyn Iterator<Item = Self::Value> + '_>;

    // -- Bit array --
    fn from_bit_array(bits: &[u8]) -> Self;
    fn to_bit_array(&self) -> Vec<u8>;

    // -- Statistics --
    fn stat_text(&self) -> String;
    fn stat_json(&self) -> String;
}
