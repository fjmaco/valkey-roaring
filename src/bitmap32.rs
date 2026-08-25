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
        // Gap-skipping walk (O(present values), matching upstream v1.7.4):
        // `candidate` is the smallest value not yet classified.
        if n == 0 {
            return None;
        }
        let mut n = n;
        let mut candidate: u64 = 0;
        for v in self.iter() {
            let v = v as u64;
            if v > candidate {
                let gap = v - candidate;
                if n <= gap {
                    return Some((candidate + n - 1) as u32);
                }
                n -= gap;
            }
            candidate = v + 1;
        }
        // Everything from `candidate` upward is absent.
        let answer = candidate.checked_add(n - 1)?;
        if answer > u32::MAX as u64 {
            None
        } else {
            Some(answer as u32)
        }
    }

    fn flip_inclusive(&self, last: u32) -> Self {
        let mut range_bm = RoaringBitmap::new();
        range_bm.insert_range(0..=last);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::xorshift;

    fn bm(vals: &[u32]) -> RoaringBitmap {
        vals.iter().copied().collect()
    }

    #[test]
    fn nth_absent_empty_bitmap() {
        let b = RoaringBitmap::new();
        assert_eq!(b.nth_absent(0), None);
        assert_eq!(b.nth_absent(1), Some(0));
        assert_eq!(b.nth_absent(5), Some(4));
    }

    #[test]
    fn nth_absent_single_zero() {
        // Upstream v1.7.4 fix: for bitmap {0} the first absent value is 1.
        assert_eq!(bm(&[0]).nth_absent(1), Some(1));
    }

    #[test]
    fn nth_absent_prefix_run() {
        assert_eq!(bm(&[0, 1, 2]).nth_absent(1), Some(3));
    }

    #[test]
    fn nth_absent_gaps() {
        let b = bm(&[1, 3, 5]);
        assert_eq!(b.nth_absent(1), Some(0));
        assert_eq!(b.nth_absent(2), Some(2));
        assert_eq!(b.nth_absent(3), Some(4));
        assert_eq!(b.nth_absent(4), Some(6));
    }

    #[test]
    fn nth_absent_type_boundary() {
        assert_eq!(bm(&[u32::MAX]).nth_absent(1), Some(0));
        // {0..=10}: the (2^32 - 11)th absent value is exactly u32::MAX,
        // and one past that falls outside the type range.
        let b: RoaringBitmap = (0..=10).collect();
        let remaining = u32::MAX as u64 - 10;
        assert_eq!(b.nth_absent(remaining), Some(u32::MAX));
        assert_eq!(b.nth_absent(remaining + 1), None);
    }

    #[test]
    fn nth_absent_matches_brute_force() {
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        for _ in 0..100 {
            let card = xorshift(&mut state) % 24;
            let vals: Vec<u32> = (0..card)
                .map(|_| (xorshift(&mut state) % 32) as u32)
                .collect();
            let b = bm(&vals);
            let absent: Vec<u32> = (0..64).filter(|v| !b.contains(*v)).collect();
            for (i, expected) in absent.iter().take(12).enumerate() {
                assert_eq!(
                    b.nth_absent(i as u64 + 1),
                    Some(*expected),
                    "bitmap {:?}, n = {}",
                    vals,
                    i + 1
                );
            }
        }
    }

    #[test]
    fn flip_inclusive_basic() {
        assert_eq!(bm(&[1, 3]).flip_inclusive(5), bm(&[0, 2, 4, 5]));
        assert_eq!(RoaringBitmap::new().flip_inclusive(3), bm(&[0, 1, 2, 3]));
        assert_eq!(bm(&[0]).flip_inclusive(0), RoaringBitmap::new());
    }

    #[test]
    fn flip_inclusive_preserves_bits_above_last() {
        assert_eq!(bm(&[1, 10]).flip_inclusive(5), bm(&[0, 2, 3, 4, 5, 10]));
    }

    #[test]
    fn flip_inclusive_full_type_range() {
        let flipped = bm(&[0]).flip_inclusive(u32::MAX);
        assert_eq!(RoaringType::len(&flipped), u32::MAX as u64); // 2^32 - 1 values
        assert!(!flipped.contains(0));
        assert!(flipped.contains(u32::MAX));
    }

    #[test]
    fn flip_inclusive_is_involutive() {
        let b = bm(&[2, 4, 9]);
        assert_eq!(b.flip_inclusive(20).flip_inclusive(20), b);
    }

    #[test]
    fn remove_many_counted_duplicates() {
        // Upstream v1.7.4 fix: duplicate offsets are counted once.
        let mut b = bm(&[5, 7]);
        assert_eq!(b.remove_many_counted(&[5, 5, 5]), 1);
        assert_eq!(b, bm(&[7]));
        assert_eq!(b.remove_many_counted(&[9]), 0);
        assert_eq!(b.remove_many_counted(&[7, 7]), 1);
        assert!(b.is_empty());
    }

    #[test]
    fn bit_array_round_trip() {
        let b = RoaringBitmap::from_bit_array(b"0101");
        assert_eq!(b, bm(&[1, 3]));
        assert_eq!(b.to_bit_array(), b"0101".to_vec());
        assert_eq!(RoaringBitmap::new().to_bit_array(), Vec::<u8>::new());
        assert_eq!(RoaringBitmap::from_bit_array(b""), RoaringBitmap::new());
        // Non-'1' bytes are treated as 0.
        assert_eq!(RoaringBitmap::from_bit_array(b"1x01"), bm(&[0, 3]));
    }

    #[test]
    fn select_bounds() {
        let b = bm(&[10, 20, 30]);
        assert_eq!(RoaringType::select(&b, 0), Some(10));
        assert_eq!(RoaringType::select(&b, 2), Some(30));
        assert_eq!(RoaringType::select(&b, 3), None);
        assert_eq!(RoaringType::select(&b, u32::MAX as u64 + 1), None);
    }
}

#[cfg(test)]
mod delegation_tests {
    use super::*;

    fn bm(vals: &[u32]) -> RoaringBitmap {
        vals.iter().copied().collect()
    }

    #[test]
    fn trait_delegation_smoke() {
        assert_eq!(
            RoaringType::len(&<RoaringBitmap as RoaringType>::full()),
            1u64 << 32
        );
        assert_eq!(RoaringBitmap::value_to_i64(7), 7);
        assert_eq!(RoaringBitmap::value_to_i64(u32::MAX), u32::MAX as i64);

        let mut b = RoaringBitmap::new();
        assert!(RoaringType::min_val(&b).is_none());
        assert!(RoaringType::max_val(&b).is_none());
        RoaringType::insert_many(&mut b, &[3, 1, 2]);
        assert_eq!(RoaringType::min_val(&b), Some(1));
        assert_eq!(RoaringType::max_val(&b), Some(3));
        assert_eq!(RoaringType::contains_many(&b, &[1, 4]), vec![true, false]);
        RoaringType::remove_many(&mut b, &[1, 9]);
        assert!(!RoaringType::contains(&b, 1));
        assert_eq!(RoaringType::insert_range_inclusive(&mut b, 10, 12), 3);
        assert_eq!(
            RoaringType::iter_range(&b, 10, 11).collect::<Vec<_>>(),
            vec![10, 11]
        );
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
        let mut d = bm(&[1]);
        let _ = RoaringType::optimize(&mut d);
        assert!(RoaringType::contains(&d, 1));
    }

    #[test]
    fn stat_output_contains_fields() {
        let b = bm(&[1, 2, 3]);
        let text = RoaringType::stat_text(&b);
        assert!(text.contains("type: bitmap"));
        assert!(text.contains("cardinality: 3"));
        assert!(text.contains("max value: 3"));
        assert!(text.contains("min value: 1"));
        let json = RoaringType::stat_json(&b);
        assert!(json.contains("\"type\":\"bitmap\""));
        assert!(json.contains("\"cardinality\":\"3\""));
        assert_eq!(json.matches('{').count(), json.matches('}').count());
    }
}
