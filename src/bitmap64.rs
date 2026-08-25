//! valkey-roaring: RoaringType implementation for RoaringTreemap (u64).

use crate::bitmap_type::RoaringType;
use roaring::RoaringTreemap;
use std::io;

/// Container statistics aggregated across a treemap's 32-bit sub-bitmaps.
#[derive(Default)]
struct TreemapStats {
    n_containers: u64,
    n_array_containers: u64,
    n_values_array_containers: u64,
    n_bytes_array_containers: u64,
    n_bitset_containers: u64,
    n_values_bitset_containers: u64,
    n_bytes_bitset_containers: u64,
    n_run_containers: u64,
    n_values_run_containers: u64,
    n_bytes_run_containers: u64,
}

fn aggregate_stats(tm: &RoaringTreemap) -> TreemapStats {
    let mut t = TreemapStats::default();
    for (_, bm) in tm.bitmaps() {
        let s = bm.statistics();
        t.n_containers += u64::from(s.n_containers);
        t.n_array_containers += u64::from(s.n_array_containers);
        t.n_values_array_containers += u64::from(s.n_values_array_containers);
        t.n_bytes_array_containers += s.n_bytes_array_containers;
        t.n_bitset_containers += u64::from(s.n_bitset_containers);
        t.n_values_bitset_containers += s.n_values_bitset_containers;
        t.n_bytes_bitset_containers += s.n_bytes_bitset_containers;
        t.n_run_containers += u64::from(s.n_run_containers);
        t.n_values_run_containers += u64::from(s.n_values_run_containers);
        t.n_bytes_run_containers += s.n_bytes_run_containers;
    }
    t
}

impl RoaringType for RoaringTreemap {
    type Value = u64;

    fn value_to_i64(v: u64) -> i64 {
        i64::try_from(v).unwrap_or(i64::MAX)
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
        // Find the nth element NOT present in the set (1-indexed).
        // Gap-skipping walk (O(present values), matching upstream v1.7.4):
        // `candidate` is the smallest value not yet classified.
        if n == 0 {
            return None;
        }
        let mut n = n;
        let mut candidate: u64 = 0;
        for v in self.iter() {
            if v > candidate {
                let gap = v - candidate;
                if n <= gap {
                    return Some(candidate + n - 1);
                }
                n -= gap;
            }
            // v == u64::MAX means no value can be absent beyond it.
            candidate = v.checked_add(1)?;
        }
        // Everything from `candidate` upward is absent.
        candidate.checked_add(n - 1)
    }

    fn flip_inclusive(&self, last: u64) -> Self {
        let mut range_bm = RoaringTreemap::new();
        range_bm.insert_range(0..=last);
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
        RoaringTreemap::optimize(self)
    }

    fn insert_range_inclusive(&mut self, start: u64, end: u64) -> u64 {
        self.insert_range(start..=end)
    }

    fn iter_values(&self) -> Box<dyn Iterator<Item = u64> + '_> {
        Box::new(self.iter())
    }

    fn iter_range(&self, start: u64, end: u64) -> Box<dyn Iterator<Item = u64> + '_> {
        // RoaringTreemap has no range() method; filter the iterator
        Box::new(
            self.iter()
                .skip_while(move |&v| v < start)
                .take_while(move |&v| v <= end),
        )
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
        // Callers guard against huge maxima (see handle_getbitarray).
        let max = self.max().unwrap();
        let mut bits = vec![b'0'; max as usize + 1];
        for v in self.iter() {
            bits[v as usize] = b'1';
        }
        bits
    }

    fn stat_text(&self) -> String {
        let s = aggregate_stats(self);
        format!(
            "type: bitmap64\n\
             cardinality: {}\n\
             number of containers: {}\n\
             max value: {}\n\
             min value: {}\n\
             serialized bytes: {}\n\
             number of array containers: {}\n\
               array container values: {}\n\
               array container bytes: {}\n\
             bitset containers: {}\n\
               bitset container values: {}\n\
               bitset container bytes: {}\n\
             run containers: {}\n\
               run container values: {}\n\
               run container bytes: {}",
            self.len(),
            s.n_containers,
            self.max().map_or("(none)".to_string(), |v| v.to_string()),
            self.min().map_or("(none)".to_string(), |v| v.to_string()),
            self.serialized_size(),
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
        let s = aggregate_stats(self);
        format!(
            "{{\"type\":\"bitmap64\",\
             \"cardinality\":\"{}\",\
             \"number_of_containers\":\"{}\",\
             \"max_value\":\"{}\",\
             \"min_value\":\"{}\",\
             \"serialized_bytes\":\"{}\",\
             \"array_container\":{{\"number_of_containers\":\"{}\",\"container_cardinality\":\"{}\",\"container_allocated_bytes\":\"{}\"}},\
             \"bitset_container\":{{\"number_of_containers\":\"{}\",\"container_cardinality\":\"{}\",\"container_allocated_bytes\":\"{}\"}},\
             \"run_container\":{{\"number_of_containers\":\"{}\",\"container_cardinality\":\"{}\",\"container_allocated_bytes\":\"{}\"}}}}",
            self.len(),
            s.n_containers,
            self.max().map_or_else(|| "null".to_string(), |v| v.to_string()),
            self.min().map_or_else(|| "null".to_string(), |v| v.to_string()),
            self.serialized_size(),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn bm(vals: &[u64]) -> RoaringTreemap {
        vals.iter().copied().collect()
    }

    #[test]
    fn value_to_i64_saturates() {
        assert_eq!(RoaringTreemap::value_to_i64(5), 5);
        assert_eq!(RoaringTreemap::value_to_i64(i64::MAX as u64), i64::MAX);
        assert_eq!(RoaringTreemap::value_to_i64(i64::MAX as u64 + 1), i64::MAX);
        assert_eq!(RoaringTreemap::value_to_i64(u64::MAX), i64::MAX);
    }

    #[test]
    fn nth_absent_basics() {
        assert_eq!(RoaringTreemap::new().nth_absent(1), Some(0));
        assert_eq!(bm(&[0]).nth_absent(1), Some(1));
        assert_eq!(bm(&[0, 1]).nth_absent(1), Some(2));
        let b = bm(&[1, 3, 5]);
        assert_eq!(b.nth_absent(1), Some(0));
        assert_eq!(b.nth_absent(3), Some(4));
    }

    #[test]
    fn nth_absent_large_gap() {
        // Gap-skipping must not iterate value-by-value across huge ranges.
        assert_eq!(bm(&[1 << 40]).nth_absent(1), Some(0));
        assert_eq!(bm(&[0, 1 << 40]).nth_absent(1), Some(1));
    }

    #[test]
    fn nth_absent_type_boundary() {
        assert_eq!(bm(&[u64::MAX]).nth_absent(1), Some(0));
        // {5, u64::MAX}: 2^64 - 2 values are absent; the last one is MAX - 1.
        let b = bm(&[5, u64::MAX]);
        assert_eq!(b.nth_absent(u64::MAX - 1), Some(u64::MAX - 1));
        assert_eq!(b.nth_absent(u64::MAX), None);
        // {0..=10}: the (2^64 - 11)th absent value is exactly u64::MAX.
        let b: RoaringTreemap = (0..=10u64).collect();
        assert_eq!(b.nth_absent(u64::MAX - 10), Some(u64::MAX));
        assert_eq!(b.nth_absent(u64::MAX - 9), None);
    }

    #[test]
    fn flip_inclusive_basic() {
        assert_eq!(bm(&[1, 3]).flip_inclusive(5), bm(&[0, 2, 4, 5]));
        assert_eq!(RoaringTreemap::new().flip_inclusive(3), bm(&[0, 1, 2, 3]));
        assert_eq!(bm(&[1, 10]).flip_inclusive(5), bm(&[0, 2, 3, 4, 5, 10]));
    }

    #[test]
    fn flip_inclusive_above_u32_range() {
        let big = 1u64 << 40;
        let b = bm(&[big]);
        let flipped = b.flip_inclusive(big + 2);
        assert!(!flipped.contains(big));
        assert!(flipped.contains(big + 1));
        assert!(flipped.contains(big + 2));
        assert_eq!(RoaringType::len(&flipped), big + 2);
    }

    #[test]
    fn optimize_preserves_data() {
        let mut b = RoaringTreemap::new();
        b.insert_range(0..=100_000);
        RoaringType::optimize(&mut b);
        assert_eq!(RoaringType::len(&b), 100_001);
        assert!(b.contains(0) && b.contains(100_000));
    }

    #[test]
    fn iter_range_filters_inclusive() {
        let b = bm(&[1, 5, 10]);
        assert_eq!(b.iter_range(2, 9).collect::<Vec<_>>(), vec![5]);
        assert_eq!(b.iter_range(1, 10).collect::<Vec<_>>(), vec![1, 5, 10]);
        assert_eq!(b.iter_range(6, 9).collect::<Vec<_>>(), Vec::<u64>::new());
    }

    #[test]
    fn remove_many_counted_duplicates() {
        let mut b = bm(&[100]);
        assert_eq!(b.remove_many_counted(&[100, 100, 100]), 1);
        assert!(b.is_empty());
    }
}

#[cfg(test)]
mod delegation_tests {
    use super::*;

    fn bm(vals: &[u64]) -> RoaringTreemap {
        vals.iter().copied().collect()
    }

    #[test]
    fn trait_delegation_smoke() {
        assert_eq!(RoaringTreemap::value_to_i64(7), 7);

        let mut b = RoaringTreemap::new();
        assert!(RoaringType::min_val(&b).is_none());
        assert!(RoaringType::max_val(&b).is_none());
        RoaringType::insert_many(&mut b, &[5_000_000_000, 1, 2]);
        assert_eq!(RoaringType::min_val(&b), Some(1));
        assert_eq!(RoaringType::max_val(&b), Some(5_000_000_000));
        assert_eq!(RoaringType::contains_many(&b, &[1, 4]), vec![true, false]);
        RoaringType::remove_many(&mut b, &[1, 9]);
        assert!(!RoaringType::contains(&b, 1));
        assert_eq!(RoaringType::insert_range_inclusive(&mut b, 10, 12), 3);
        assert_eq!(
            RoaringType::iter_values(&b).count() as u64,
            RoaringType::len(&b)
        );

        let other = bm(&[2, 100]);
        assert!(!RoaringType::is_disjoint(&b, &other));
        assert!(RoaringType::is_subset(&bm(&[2]), &other));
        assert_eq!(RoaringType::intersection_len(&b, &other), 1);
        assert_eq!(RoaringType::union_len(&b, &other), RoaringType::len(&b) + 1);
        assert_eq!(RoaringType::sub_owned(bm(&[1, 2]), bm(&[2])), bm(&[1]));

        let mut c = bm(&[1, 2]);
        RoaringType::clear(&mut c);
        assert!(c.is_empty());
    }

    #[test]
    fn stat_output_contains_fields() {
        let b = bm(&[1, 5_000_000_000]);
        let text = RoaringType::stat_text(&b);
        assert!(text.contains("type: bitmap64"));
        assert!(text.contains("cardinality: 2"));
        assert!(text.contains("max value: 5000000000"));
        // Container breakdown aggregated across sub-bitmaps: the two values
        // land in different 32-bit partitions -> two array containers.
        assert!(text.contains("number of containers: 2"));
        assert!(text.contains("number of array containers: 2"));
        let json = RoaringType::stat_json(&b);
        assert!(json.contains("\"type\":\"bitmap64\""));
        assert!(json.contains("\"max_value\":\"5000000000\""));
        assert!(json.contains("\"number_of_containers\":\"2\""));
        assert!(json.contains("\"array_container\""));
        assert_eq!(json.matches('{').count(), json.matches('}').count());
        // Empty treemap exercises the none/null formatting branches.
        let e = RoaringTreemap::new();
        assert!(RoaringType::stat_text(&e).contains("(none)"));
        assert!(RoaringType::stat_json(&e).contains("null"));
    }

    #[test]
    fn bit_array_round_trip() {
        let b = RoaringTreemap::from_bit_array(b"0011");
        assert_eq!(b, bm(&[2, 3]));
        assert_eq!(RoaringType::to_bit_array(&b), b"0011".to_vec());
        assert_eq!(
            RoaringType::to_bit_array(&RoaringTreemap::new()),
            Vec::<u8>::new()
        );
    }

    #[test]
    fn serialized_size_matches_output() {
        let b = bm(&[1, 2, 3, 1 << 40]);
        let mut buf = Vec::new();
        RoaringType::serialize_into(&b, &mut buf).unwrap();
        assert_eq!(buf.len(), RoaringType::serialized_size(&b));
    }
}
