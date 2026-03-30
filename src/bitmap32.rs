//! valkey-roaring: RoaringType implementation for RoaringBitmap (u32).

use crate::bitmap_type::RoaringType;
use roaring::RoaringBitmap;
use std::io;

impl RoaringType for RoaringBitmap {
    type Value = u32;

    fn value_to_i64(v: u32) -> i64 {
        v as i64
    }

    fn new() -> Self {
        RoaringBitmap::new()
    }

    fn full() -> Self {
        RoaringBitmap::full()
    }

    fn from_values(vals: &[u32]) -> Self {
        vals.iter().copied().collect()
    }

    fn from_range_inclusive(start: u32, end: u32) -> Self {
        let mut bm = RoaringBitmap::new();
        bm.insert_range(start..=end);
        bm
    }

    fn insert(&mut self, v: u32) -> bool {
        RoaringBitmap::insert(self, v)
    }

    fn remove(&mut self, v: u32) -> bool {
        RoaringBitmap::remove(self, v)
    }

    fn contains(&self, v: u32) -> bool {
        RoaringBitmap::contains(self, v)
    }

    fn clear(&mut self) {
        RoaringBitmap::clear(self);
    }

    fn insert_many(&mut self, vals: &[u32]) {
        for &v in vals {
            self.insert(v);
        }
    }

    fn remove_many(&mut self, vals: &[u32]) {
        for &v in vals {
            self.remove(v);
        }
    }

    fn contains_many(&self, vals: &[u32]) -> Vec<bool> {
        vals.iter().map(|&v| self.contains(v)).collect()
    }

    fn remove_many_counted(&mut self, vals: &[u32]) -> usize {
        vals.iter().filter(|&&v| self.remove(v)).count()
    }

    fn len(&self) -> u64 {
        RoaringBitmap::len(self)
    }

    fn is_empty(&self) -> bool {
        RoaringBitmap::is_empty(self)
    }

    fn min_val(&self) -> Option<u32> {
        self.min()
    }

    fn max_val(&self) -> Option<u32> {
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
        RoaringBitmap::is_disjoint(self, other)
    }

    fn is_subset(&self, other: &Self) -> bool {
        RoaringBitmap::is_subset(self, other)
    }

    fn intersection_len(&self, other: &Self) -> u64 {
        RoaringBitmap::intersection_len(self, other)
    }

    fn union_len(&self, other: &Self) -> u64 {
        RoaringBitmap::union_len(self, other)
    }

    fn select(&self, n: u64) -> Option<u32> {
        if n > u32::MAX as u64 {
            return None;
        }
        RoaringBitmap::select(self, n as u32)
    }

    fn nth_absent(&self, n: u64) -> Option<u32> {
        // Find the nth element NOT present in the set (1-indexed).
        if n == 0 {
            return None;
        }
        let mut count = 0u64;
        let mut candidate = 0u64;
        let mut iter = self.iter().peekable();
        loop {
            if candidate > u32::MAX as u64 {
                return None;
            }
            match iter.peek() {
                Some(&v) if (v as u64) == candidate => {
                    iter.next();
                    candidate += 1;
                }
                _ => {
                    count += 1;
                    if count == n {
                        return Some(candidate as u32);
                    }
                    candidate += 1;
                }
            }
        }
    }

    fn flip_to(&self, end_exclusive: u32) -> Self {
        if end_exclusive == 0 {
            return RoaringBitmap::new();
        }
        let mut range_bm = RoaringBitmap::new();
        range_bm.insert_range(0..end_exclusive);
        range_bm ^= self.clone();
        range_bm
    }

    fn serialize_into<W: io::Write>(&self, writer: W) -> io::Result<()> {
        RoaringBitmap::serialize_into(self, writer)
    }

    fn deserialize_from<R: io::Read>(reader: R) -> io::Result<Self> {
        RoaringBitmap::deserialize_from(reader)
    }

    fn serialized_size(&self) -> usize {
        RoaringBitmap::serialized_size(self)
    }

    fn optimize(&mut self) -> bool {
        RoaringBitmap::optimize(self)
    }

    fn insert_range_inclusive(&mut self, start: u32, end: u32) -> u64 {
        self.insert_range(start..=end)
    }

    fn iter_values(&self) -> Box<dyn Iterator<Item = u32> + '_> {
        Box::new(self.iter())
    }

    fn iter_range(&self, start: u32, end: u32) -> Box<dyn Iterator<Item = u32> + '_> {
        Box::new(self.range(start..=end))
    }

    fn from_bit_array(bits: &[u8]) -> Self {
        let mut bm = RoaringBitmap::new();
        for (i, &b) in bits.iter().enumerate() {
            if b == b'1' {
                bm.insert(i as u32);
            }
        }
        bm
    }

    fn to_bit_array(&self) -> Vec<u8> {
        if self.is_empty() {
            return Vec::new();
        }
        let max = self.max().unwrap();
        let mut bits = vec![b'0'; max as usize + 1];
        for v in self.iter() {
            bits[v as usize] = b'1';
        }
        bits
    }

    fn stat_text(&self) -> String {
        let s = self.statistics();
        format!(
            "type: bitmap\n\
             cardinality: {}\n\
             number of containers: {}\n\
             max value: {}\n\
             min value: {}\n\
             number of array containers: {}\n\
               array container values: {}\n\
               array container bytes: {}\n\
             bitset containers: {}\n\
               bitset container values: {}\n\
               bitset container bytes: {}\n\
             run containers: {}\n\
               run container values: {}\n\
               run container bytes: {}",
            s.cardinality,
            s.n_containers,
            s.max_value.map_or(0, |v| v),
            s.min_value.map_or(0, |v| v),
            s.n_array_containers,
            s.n_values_array_containers,
            s.n_bytes_array_containers,
            s.n_bitset_containers,
            s.n_values_bitset_containers,
            s.n_bytes_bitset_containers,
            s.n_run_containers,
            s.n_values_run_containers,
            s.n_bytes_run_containers,
        )
    }

    fn stat_json(&self) -> String {
        let s = self.statistics();
        format!(
            "{{\"type\":\"bitmap\",\
             \"cardinality\":\"{}\",\
             \"number_of_containers\":\"{}\",\
             \"max_value\":\"{}\",\
             \"min_value\":\"{}\",\
             \"array_container\":{{\"number_of_containers\":\"{}\",\"container_cardinality\":\"{}\",\"container_allocated_bytes\":\"{}\"}},\
             \"bitset_container\":{{\"number_of_containers\":\"{}\",\"container_cardinality\":\"{}\",\"container_allocated_bytes\":\"{}\"}},\
             \"run_container\":{{\"number_of_containers\":\"{}\",\"container_cardinality\":\"{}\",\"container_allocated_bytes\":\"{}\"}}}}",
            s.cardinality,
            s.n_containers,
            s.max_value.map_or(0, |v| v),
            s.min_value.map_or(0, |v| v),
            s.n_array_containers,
            s.n_values_array_containers,
            s.n_bytes_array_containers,
            s.n_bitset_containers,
            s.n_values_bitset_containers,
            s.n_bytes_bitset_containers,
            s.n_run_containers,
            s.n_values_run_containers,
            s.n_bytes_run_containers,
        )
    }
}
