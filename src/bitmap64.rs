//! valkey-roaring: RoaringType implementation for RoaringTreemap (u64).

use crate::bitmap_type::RoaringType;
use roaring::RoaringTreemap;
use std::io;

impl RoaringType for RoaringTreemap {
    type Value = u64;

    fn value_to_i64(v: u64) -> i64 {
        v as i64
    }

    fn new() -> Self {
        RoaringTreemap::new()
    }

    fn full() -> Self {
        RoaringTreemap::full()
    }

    fn from_values(vals: &[u64]) -> Self {
        vals.iter().copied().collect()
    }

    fn from_range_inclusive(start: u64, end: u64) -> Self {
        let mut bm = RoaringTreemap::new();
        bm.insert_range(start..=end);
        bm
    }

    fn insert(&mut self, v: u64) -> bool {
        RoaringTreemap::insert(self, v)
    }

    fn remove(&mut self, v: u64) -> bool {
        RoaringTreemap::remove(self, v)
    }

    fn contains(&self, v: u64) -> bool {
        RoaringTreemap::contains(self, v)
    }

    fn clear(&mut self) {
        RoaringTreemap::clear(self);
    }

    fn insert_many(&mut self, vals: &[u64]) {
        for &v in vals {
            self.insert(v);
        }
    }

    fn remove_many(&mut self, vals: &[u64]) {
        for &v in vals {
            self.remove(v);
        }
    }

    fn contains_many(&self, vals: &[u64]) -> Vec<bool> {
        vals.iter().map(|&v| self.contains(v)).collect()
    }

    fn remove_many_counted(&mut self, vals: &[u64]) -> usize {
        vals.iter().filter(|&&v| self.remove(v)).count()
    }

    fn len(&self) -> u64 {
        RoaringTreemap::len(self)
    }

    fn is_empty(&self) -> bool {
        RoaringTreemap::is_empty(self)
    }

    fn min_val(&self) -> Option<u64> {
        self.min()
    }

    fn max_val(&self) -> Option<u64> {
        self.max()
    }

    fn bitor_assign(&mut self, other: &Self) {
        *self |= other.clone();
    }

    fn bitand_assign(&mut self, other: &Self) {
        *self &= other.clone();
    }

    fn bitxor_assign(&mut self, other: &Self) {
        *self ^= other.clone();
    }

    fn sub_assign(&mut self, other: &Self) {
        *self -= other.clone();
    }

    fn bitor_owned(self, other: Self) -> Self {
        self | other
    }

    fn bitand_owned(self, other: Self) -> Self {
        self & other
    }

    fn bitxor_owned(self, other: Self) -> Self {
        self ^ other
    }

    fn sub_owned(self, other: Self) -> Self {
        self - other
    }

    fn is_disjoint(&self, other: &Self) -> bool {
        RoaringTreemap::is_disjoint(self, other)
    }

    fn is_subset(&self, other: &Self) -> bool {
        RoaringTreemap::is_subset(self, other)
    }

    fn intersection_len(&self, other: &Self) -> u64 {
        RoaringTreemap::intersection_len(self, other)
    }

    fn union_len(&self, other: &Self) -> u64 {
        RoaringTreemap::union_len(self, other)
    }

    fn select(&self, n: u64) -> Option<u64> {
        RoaringTreemap::select(self, n)
    }

    fn nth_absent(&self, n: u64) -> Option<u64> {
        if n == 0 {
            return None;
        }
        let mut count = 0u64;
        let mut candidate = 0u64;
        let mut iter = self.iter().peekable();
        loop {
            match iter.peek() {
                Some(&v) if v == candidate => {
                    iter.next();
                    candidate = candidate.checked_add(1)?;
                }
                _ => {
                    count += 1;
                    if count == n {
                        return Some(candidate);
                    }
                    candidate = candidate.checked_add(1)?;
                }
            }
        }
    }

    fn flip_to(&self, end_exclusive: u64) -> Self {
        if end_exclusive == 0 {
            return RoaringTreemap::new();
        }
        let mut range_bm = RoaringTreemap::new();
        range_bm.insert_range(0..end_exclusive);
        range_bm ^= self.clone();
        range_bm
    }

    fn serialize_into<W: io::Write>(&self, writer: W) -> io::Result<()> {
        RoaringTreemap::serialize_into(self, writer)
    }

    fn deserialize_from<R: io::Read>(reader: R) -> io::Result<Self> {
        RoaringTreemap::deserialize_from(reader)
    }

    fn serialized_size(&self) -> usize {
        RoaringTreemap::serialized_size(self)
    }

    fn optimize(&mut self) -> bool {
        // RoaringTreemap has no optimize(); it auto-optimizes per internal bitmap.
        false
    }

    fn insert_range_inclusive(&mut self, start: u64, end: u64) -> u64 {
        self.insert_range(start..=end)
    }

    fn iter_values(&self) -> Box<dyn Iterator<Item = u64> + '_> {
        Box::new(self.iter())
    }

    fn iter_range(&self, start: u64, end: u64) -> Box<dyn Iterator<Item = u64> + '_> {
        // RoaringTreemap has no range() method; filter the iterator
        Box::new(self.iter().skip_while(move |&v| v < start).take_while(move |&v| v <= end))
    }

    fn from_bit_array(bits: &[u8]) -> Self {
        let mut bm = RoaringTreemap::new();
        for (i, &b) in bits.iter().enumerate() {
            if b == b'1' {
                bm.insert(i as u64);
            }
        }
        bm
    }

    fn to_bit_array(&self) -> Vec<u8> {
        if self.is_empty() {
            return Vec::new();
        }
        let max = self.max().unwrap();
        // Cap at reasonable size to avoid OOM
        if max > 100_000_000 {
            return Vec::new();
        }
        let mut bits = vec![b'0'; max as usize + 1];
        for v in self.iter() {
            bits[v as usize] = b'1';
        }
        bits
    }

    fn stat_text(&self) -> String {
        // RoaringTreemap has no statistics() — use public API only
        format!(
            "type: bitmap64\n\
             cardinality: {}\n\
             max value: {}\n\
             min value: {}\n\
             serialized bytes: {}",
            self.len(),
            self.max().map_or("(none)".to_string(), |v| v.to_string()),
            self.min().map_or("(none)".to_string(), |v| v.to_string()),
            self.serialized_size(),
        )
    }

    fn stat_json(&self) -> String {
        format!(
            "{{\"type\":\"bitmap64\",\
             \"cardinality\":\"{}\",\
             \"max_value\":\"{}\",\
             \"min_value\":\"{}\",\
             \"serialized_bytes\":\"{}\"}}",
            self.len(),
            self.max().map_or_else(|| "null".to_string(), |v| v.to_string()),
            self.min().map_or_else(|| "null".to_string(), |v| v.to_string()),
            self.serialized_size(),
        )
    }
}
