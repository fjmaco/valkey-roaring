//! valkey-roaring: RoaringType implementation for RoaringTreemap (u64).

use crate::bitmap32::{
    alloc_size, bitmaps_equal, containers_heap_size, from_sorted_exact, tied_values,
    union_in_place_is_cheap, worth_compacting, INTERSECT_IN_PLACE_FROM,
};
use crate::bitmap_type::{RoaringType, StatFields};
use crate::canonical::{self, Tie};
use crate::parse;
use roaring::{RoaringBitmap, RoaringTreemap};
use std::io;
use std::mem::size_of;

/// A treemap keeps its sub-bitmaps in a BTreeMap<u32, RoaringBitmap>. Leaf
/// nodes hold up to 11 entries in 320 bytes; internal nodes add 12 child
/// pointers (416 bytes, 448 once allocated). Measured, roaring 0.11.5 on a
/// 64-bit target. One leaf holds up to 11 sub-bitmaps; past that, nodes
/// split and run about two-thirds full.
const BTREE_LEAF_BYTES: usize = 320;
const BTREE_INTERNAL_BYTES: usize = 448;
const BTREE_LEAF_ENTRIES: usize = 11;
const BTREE_ENTRIES_PER_NODE: usize = 8;

fn btree_bytes(entries: usize) -> usize {
    match entries {
        0 => 0,
        1..=BTREE_LEAF_ENTRIES => BTREE_LEAF_BYTES,
        _ => {
            let leaves = entries.div_ceil(BTREE_ENTRIES_PER_NODE);
            leaves * BTREE_LEAF_BYTES
                + leaves.div_ceil(BTREE_ENTRIES_PER_NODE) * BTREE_INTERNAL_BYTES
        }
    }
}

/// Container statistics aggregated across a treemap's 32-bit sub-bitmaps.
#[derive(Default)]
struct TreemapStats {
    n_containers: u64,
    n_array_containers: u64,
    n_values_array_containers: u64,
    n_bitset_containers: u64,
    n_values_bitset_containers: u64,
    n_run_containers: u64,
    n_bytes_run_containers: u64,
}

/// Estimated heap bytes and reclaimable growth slack (see bitmap32's
/// containers_heap_size). Sub-bitmap structs live inside the BTreeMap's
/// nodes, so only their containers count on top of the nodes.
fn treemap_heap_size(tm: &RoaringTreemap) -> (usize, usize, usize) {
    let (mut entries, mut heap, mut slack, mut containers) = (0usize, 0usize, 0usize, 0usize);
    for (_, bm) in tm.bitmaps() {
        let (h, s, n) = containers_heap_size(bm);
        entries += 1;
        heap += h;
        slack += s;
        containers += n;
    }
    let own = alloc_size(size_of::<RoaringTreemap>());
    (own + btree_bytes(entries) + heap, slack, containers)
}

/// RoaringTreemap::serialize_into's layout (a u64 sub-bitmap count, then per
/// sub-bitmap its high 32 bits and a 32-bit portable blob), written here so
/// each sub-blob's header is checked for tied run containers right after it
/// is written: one cookie read for a sub-bitmap without runs. Returns the
/// tied containers, grouped by sub-bitmap (both in order).
type TiedContainers = Vec<(u32, Vec<Tie>)>;

fn serialize_noting_ties(tm: &RoaringTreemap) -> io::Result<(Vec<u8>, TiedContainers)> {
    let mut buf = Vec::with_capacity(tm.serialized_size());
    buf.extend_from_slice(&(tm.bitmaps().count() as u64).to_le_bytes());
    let mut tied = Vec::new();
    for (hi, bm) in tm.bitmaps() {
        buf.extend_from_slice(&hi.to_le_bytes());
        let start = buf.len();
        bm.serialize_into(&mut buf)?;
        let sub = &buf[start..];
        if !canonical::has_no_runs(sub) {
            let ties = canonical::tied_run_containers(sub).unwrap_or_default();
            if !ties.is_empty() {
                tied.push((hi, ties));
            }
        }
    }
    Ok((buf, tied))
}

/// Re-encode the tied containers as arrays, as bitmap32's demote_to_arrays
/// does: their values, gathered sub-bitmap by sub-bitmap, come out with
/// `^= &values` and go back in with `^= values`. The treemap applies each
/// per affected sub-bitmap (a B-tree lookup, then one merge), so the work is
/// linear in what changes. (Per-tie remove_range walked every sub-bitmap.)
fn demote_to_arrays(tm: &mut RoaringTreemap, tied: &[(u32, Vec<Tie>)]) {
    let mut subs = tm.bitmaps();
    let values = RoaringTreemap::from_bitmaps(tied.iter().filter_map(|(hi, ties)| {
        let (_, bm) = subs.find(|&(h, _)| h == *hi)?;
        Some((*hi, tied_values(bm, ties)))
    }));
    if values.len() == tm.len() {
        // Every container was tied: the arrays are the whole treemap.
        *tm = values;
        return;
    }
    *tm ^= &values;
    *tm ^= values;
}

/// `tm |= other` in time linear in both sizes. roaring-rs's treemap `|` and
/// `|=` both end in RoaringBitmap's `|=` for each shared sub-bitmap, which
/// inserts missing containers one at a time (bitmap32::union_in_place_is_cheap).
/// In place while that stays cheap for every shared sub-bitmap (adding a
/// whole sub-bitmap is a B-tree insert); otherwise one merge over both
/// sub-bitmap sequences.
fn union_into(tm: &mut RoaringTreemap, other: &RoaringTreemap) {
    if union_in_place_is_cheap_64(tm, other) {
        *tm |= other;
    } else {
        *tm = treemap_union(tm, other);
    }
}

fn union_in_place_is_cheap_64(tm: &RoaringTreemap, other: &RoaringTreemap) -> bool {
    let mut mine = tm.bitmaps().peekable();
    other.bitmaps().all(|(hi, theirs)| {
        while mine.next_if(|&(h, _)| h < hi).is_some() {}
        match mine.peek() {
            Some(&(h, bm)) if h == hi => union_in_place_is_cheap(bm, theirs),
            _ => true,
        }
    })
}

/// Union by a merge over both sub-bitmap sequences: shared high words are
/// joined with RoaringBitmap's one-pass by-reference `|`, the rest copied.
fn treemap_union(a: &RoaringTreemap, b: &RoaringTreemap) -> RoaringTreemap {
    use std::cmp::Ordering;
    let (mut a, mut b) = (a.bitmaps().peekable(), b.bitmaps().peekable());
    RoaringTreemap::from_bitmaps(std::iter::from_fn(|| match (a.peek(), b.peek()) {
        (Some(&(ka, ba)), Some(&(kb, bb))) => match ka.cmp(&kb) {
            Ordering::Less => a.next().map(|(k, bm)| (k, bm.clone())),
            Ordering::Greater => b.next().map(|(k, bm)| (k, bm.clone())),
            Ordering::Equal => {
                a.next();
                b.next();
                Some((ka, ba | bb))
            }
        },
        (Some(_), None) => a.next().map(|(k, bm)| (k, bm.clone())),
        (None, Some(_)) => b.next().map(|(k, bm)| (k, bm.clone())),
        (None, None) => None,
    }))
}

/// A treemap from sorted, deduplicated values, each sub-bitmap built at
/// exact size (bitmap32::from_sorted_exact).
fn from_sorted_exact_64(vals: &[u64]) -> RoaringTreemap {
    let mut subs = Vec::new();
    let mut lows = Vec::new();
    let mut start = 0;
    while start < vals.len() {
        let hi = vals[start] >> 32;
        let len = vals[start..].partition_point(|&v| v >> 32 == hi);
        lows.clear();
        lows.extend(vals[start..start + len].iter().map(|&v| v as u32));
        subs.push((hi as u32, from_sorted_exact(&lows)));
        start += len;
    }
    RoaringTreemap::from_bitmaps(subs)
}

fn aggregate_stats(tm: &RoaringTreemap) -> TreemapStats {
    let mut t = TreemapStats::default();
    for (_, bm) in tm.bitmaps() {
        let s = bm.statistics();
        t.n_containers += u64::from(s.n_containers);
        t.n_array_containers += u64::from(s.n_array_containers);
        t.n_values_array_containers += u64::from(s.n_values_array_containers);
        t.n_bitset_containers += u64::from(s.n_bitset_containers);
        t.n_values_bitset_containers += s.n_values_bitset_containers;
        t.n_run_containers += u64::from(s.n_run_containers);
        t.n_bytes_run_containers += s.n_bytes_run_containers;
    }
    t
}

impl RoaringType for RoaringTreemap {
    type Value = u64;

    const MAX_VALUE: u64 = u64::MAX;
    const VALUE_DESCRIPTION: &'static str = "must be an unsigned 64 bit integer";
    const ERR_END_BEFORE_START: &'static str = "ERR invalid end: must >= start";
    const GETBIT_PARSES_BEFORE_KEY_CHECK: bool = true;

    fn parse_value_arg(bytes: &[u8]) -> Option<u64> {
        parse::parse_u64_bytes(bytes)
    }

    fn value_to_i64(v: u64) -> i64 {
        i64::try_from(v).unwrap_or(i64::MAX)
    }

    fn value_to_u64(v: u64) -> u64 {
        v
    }

    fn new() -> Self {
        RoaringTreemap::new()
    }

    fn full() -> Self {
        RoaringTreemap::full()
    }

    fn from_values(mut vals: Vec<u64>) -> Self {
        // Already-sorted input (the common case) sorts in linear time.
        vals.sort_unstable();
        vals.dedup();
        from_sorted_exact_64(&vals)
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

    fn insert_many(&mut self, vals: &[u64]) -> usize {
        vals.iter().filter(|&&v| self.insert(v)).count()
    }

    fn remove_many(&mut self, vals: &[u64]) -> usize {
        vals.iter().filter(|&&v| self.remove(v)).count()
    }

    fn contains_range_exclusive(&self, start: u64, end: u64) -> bool {
        // roaring-rs visits only the sub-bitmaps under the range's high
        // words (a BTreeMap range), each through RoaringBitmap's
        // O(log containers + span) check: the cost follows the range, never
        // the size of the key. (An empty range is contained.)
        self.contains_range(start..end)
    }

    fn len(&self) -> u64 {
        RoaringTreemap::len(self)
    }

    fn container_count(&self) -> usize {
        self.bitmaps()
            .map(|(_, bm)| bm.statistics().n_containers as usize)
            .sum()
    }

    fn min_val(&self) -> Option<u64> {
        self.min()
    }

    fn max_val(&self) -> Option<u64> {
        self.max()
    }

    fn bitor_assign(&mut self, other: &Self) {
        union_into(self, other);
    }

    fn bitand_assign(&mut self, other: &Self) {
        *self &= other;
    }

    fn bitxor_assign(&mut self, other: &Self) {
        *self ^= other;
    }

    fn sub_assign(&mut self, other: &Self) {
        *self -= other;
    }

    fn bitor_assign_owned(&mut self, other: Self) {
        // In place, moving sub-bitmaps, when that stays cheap (union_into).
        if union_in_place_is_cheap_64(self, &other) {
            *self |= other;
        } else {
            *self = treemap_union(self, &other);
        }
    }

    fn union(&self, other: &Self) -> Self {
        treemap_union(self, other)
    }

    fn intersection(&self, other: &Self) -> Self {
        // Same crossover as the 32-bit type.
        if RoaringTreemap::len(self) < INTERSECT_IN_PLACE_FROM * self.container_count() as u64 {
            self & other
        } else {
            let mut out = self.clone();
            out &= other;
            out
        }
    }

    fn symmetric_difference(&self, other: &Self) -> Self {
        self ^ other
    }

    fn difference(&self, other: &Self) -> Self {
        // Copy-then-subtract, as for the 32-bit type.
        let mut out = self.clone();
        out -= other;
        out
    }

    fn is_disjoint(&self, other: &Self) -> bool {
        RoaringTreemap::is_disjoint(self, other)
    }

    fn set_eq(&self, other: &Self) -> bool {
        // Sub-bitmaps pair up by high word (empty ones are never kept).
        let (mut a, mut b) = (self.bitmaps(), other.bitmaps());
        loop {
            match (a.next(), b.next()) {
                (None, None) => return true,
                (Some((ka, ba)), Some((kb, bb))) if ka == kb && bitmaps_equal(ba, bb) => {}
                _ => return false,
            }
        }
    }

    fn is_subset(&self, other: &Self) -> bool {
        RoaringTreemap::is_subset(self, other)
    }

    fn intersection_len(&self, other: &Self) -> u64 {
        RoaringTreemap::intersection_len(self, other)
    }

    fn select(&self, n: u64) -> Option<u64> {
        RoaringTreemap::select(self, n)
    }

    fn nth_absent(&self, n: u64) -> Option<u64> {
        // Find the nth element NOT present in the set (1-indexed).
        // Gap-skipping walk over runs of consecutive values (O(runs), where
        // upstream v1.7.4 walks values): `candidate` is the smallest value
        // not yet classified. The treemap iterator has no run API, so walk
        // each 32-bit sub-bitmap's runs; a run cut at a sub-bitmap border
        // just shows up as two adjacent runs.
        if n == 0 {
            return None;
        }
        let mut n = n;
        let mut candidate: u64 = 0;
        for (hi, bm) in self.bitmaps() {
            let base = u64::from(hi) << 32;
            let mut it = bm.iter();
            while let Some(run) = it.next_range() {
                let (start, end) = (base | u64::from(*run.start()), base | u64::from(*run.end()));
                if start > candidate {
                    let gap = start - candidate;
                    if n <= gap {
                        return Some(candidate + n - 1);
                    }
                    n -= gap;
                }
                // end == u64::MAX means no value can be absent beyond it.
                candidate = end.checked_add(1)?;
            }
        }
        // Everything from `candidate` upward is absent.
        candidate.checked_add(n - 1)
    }

    fn flip_inclusive(&self, last: u64) -> Self {
        let mut range_bm = RoaringTreemap::new();
        range_bm.insert_range(0..=last);
        range_bm ^= self;
        range_bm
    }

    fn serialize_into<W: io::Write>(&self, writer: W) -> io::Result<()> {
        RoaringTreemap::serialize_into(self, writer)
    }

    fn deserialize_from<R: io::Read>(mut reader: R) -> io::Result<Self> {
        // The portable 64-bit layout: a u64 count, then per sub-bitmap its
        // high 32 bits and a 32-bit blob. Read here rather than by
        // RoaringTreemap::deserialize_from, which keeps the last of repeated
        // high words (silently dropping values) and keeps empty sub-bitmaps.
        // High words must strictly increase, as CRoaring requires; empty
        // sub-bitmaps are valid but dropped, since treemap equality and
        // subset tests compare sub-bitmaps key by key.
        let mut word = [0u8; 8];
        reader.read_exact(&mut word)?;
        let count = u64::from_le_bytes(word);
        let mut subs = Vec::new();
        let mut previous: Option<u32> = None;
        for _ in 0..count {
            let mut hi = [0u8; 4];
            reader.read_exact(&mut hi)?;
            let hi = u32::from_le_bytes(hi);
            if previous.is_some_and(|p| hi <= p) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "sub-bitmap high words must strictly increase",
                ));
            }
            previous = Some(hi);
            let bm = RoaringBitmap::deserialize_from(&mut reader)?;
            if !bm.is_empty() {
                subs.push((hi, bm));
            }
        }
        Ok(RoaringTreemap::from_bitmaps(subs))
    }

    fn deserialize_legacy(bytes: &[u8]) -> io::Result<Self> {
        // roaring-rs's own decoder, as 1.1.1 used it: each entry is inserted
        // into the BTreeMap, so a repeated high word keeps the last one.
        let tm = RoaringTreemap::deserialize_from(bytes)?;
        if tm.bitmaps().any(|(_, bm)| bm.is_empty()) {
            return Ok(RoaringTreemap::from_bitmaps(
                tm.bitmaps()
                    .filter(|(_, bm)| !bm.is_empty())
                    .map(|(hi, bm)| (hi, bm.clone())),
            ));
        }
        Ok(tm)
    }

    fn serialized_size(&self) -> usize {
        RoaringTreemap::serialized_size(self)
    }

    fn export_canonical(&mut self) -> io::Result<Vec<u8>> {
        RoaringTreemap::optimize(self); // also prunes empty sub-bitmaps
        let (buf, tied) = serialize_noting_ties(self)?;
        if tied.is_empty() {
            return Ok(buf);
        }
        demote_to_arrays(self, &tied);
        let mut buf = buf;
        buf.clear();
        RoaringTreemap::serialize_into(self, &mut buf)?;
        Ok(buf)
    }

    fn optimize(&mut self) -> bool {
        RoaringTreemap::optimize(self)
    }

    fn heap_size(&self) -> usize {
        treemap_heap_size(self).0
    }

    fn trim(&mut self) {
        let (heap, slack, containers) = treemap_heap_size(self);
        if worth_compacting(heap, slack, containers) {
            self.compact();
        }
    }

    fn insert_range_exclusive(&mut self, start: u64, end: u64) -> u64 {
        self.insert_range(start..end)
    }

    type Values<'a> = roaring::treemap::Iter<'a>;

    fn iter_values(&self) -> Self::Values<'_> {
        self.iter()
    }

    fn iter_from(&self, start: u64) -> Self::Values<'_> {
        let mut it = self.iter();
        it.advance_to(start);
        it
    }

    fn from_bit_array(bits: &[u8]) -> Self {
        // Positions come out ascending: built at exact size in one go.
        let ones: Vec<u64> = bits
            .iter()
            .enumerate()
            .filter(|&(_, &b)| b == b'1')
            .map(|(i, _)| i as u64)
            .collect();
        from_sorted_exact_64(&ones)
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

    fn stat_fields(&self) -> StatFields {
        let s = aggregate_stats(self);
        let cardinality = self.len();
        StatFields {
            kind: "bitmap64",
            cardinality,
            containers: s.n_containers,
            max: self.max().unwrap_or(0),
            min: self.min().unwrap_or(u64::MAX),
            arrays: s.n_array_containers,
            array_values: s.n_values_array_containers,
            array_bytes: 2 * s.n_values_array_containers,
            bitsets: s.n_bitset_containers,
            bitset_values: s.n_values_bitset_containers,
            bitset_bytes: 8192 * s.n_bitset_containers,
            runs: s.n_run_containers,
            // Counted as the rest: a full sub-bitmap's run values overflow
            // roaring-rs's per-bitmap u32 counter.
            run_values: cardinality - s.n_values_array_containers - s.n_values_bitset_containers,
            run_bytes: s.n_bytes_run_containers,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::xorshift;

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
    fn from_values_sorts_and_dedups() {
        let big = 1u64 << 40;
        let b = RoaringTreemap::from_values(vec![big, 3, u64::MAX, 3, big, 0]);
        assert_eq!(b, bm(&[0, 3, big, u64::MAX]));
        assert_eq!(RoaringTreemap::from_values(vec![]), RoaringTreemap::new());
    }

    #[test]
    fn iter_from_crosses_sub_bitmaps() {
        let hi = |h: u64, lo: u64| h << 32 | lo;
        let b = bm(&[1, hi(1, 0), hi(1, 9), hi(5, 2), u64::MAX]);
        let from = |v| RoaringType::iter_from(&b, v).collect::<Vec<_>>();
        assert_eq!(from(0), vec![1, hi(1, 0), hi(1, 9), hi(5, 2), u64::MAX]);
        assert_eq!(from(2), vec![hi(1, 0), hi(1, 9), hi(5, 2), u64::MAX]);
        assert_eq!(from(hi(1, 1)), vec![hi(1, 9), hi(5, 2), u64::MAX]);
        assert_eq!(from(hi(3, 0)), vec![hi(5, 2), u64::MAX]);
        assert_eq!(from(u64::MAX), vec![u64::MAX]);
        assert!(RoaringType::iter_from(&RoaringTreemap::new(), 7)
            .next()
            .is_none());
    }

    #[test]
    fn nth_absent_runs_across_sub_bitmaps() {
        // A run straddling the 2^32 border is split across two sub-bitmaps;
        // the walk must treat the halves as one run.
        let border = 1u64 << 32;
        let mut b = RoaringTreemap::new();
        b.insert_range(0..border + 10);
        assert_eq!(b.nth_absent(1), Some(border + 10));
        b.remove(border);
        assert_eq!(b.nth_absent(1), Some(border));
        assert_eq!(b.nth_absent(2), Some(border + 10));
        // Brute force on a small domain around the border.
        let mut state = 0xA076_1D64_78BD_642Fu64;
        for _ in 0..100 {
            let vals: Vec<u64> = (0..xorshift(&mut state) % 40)
                .map(|_| border - 20 + xorshift(&mut state) % 40)
                .collect();
            let mut t = bm(&vals);
            t.insert_range(0..border - 20);
            let absent: Vec<u64> = (border - 20..border + 40)
                .filter(|v| !t.contains(*v))
                .collect();
            for (i, expected) in absent.iter().take(8).enumerate() {
                assert_eq!(t.nth_absent(i as u64 + 1), Some(*expected), "{:?}", vals);
            }
        }
    }

    #[test]
    fn intersection_matches_operator_on_both_strategies() {
        let sparse_a: RoaringTreemap = (0..100u64).map(|i| i << 20).collect();
        let sparse_b: RoaringTreemap = (0..100u64)
            .filter(|i| i % 3 == 0)
            .map(|i| i << 20)
            .collect();
        assert_eq!(
            RoaringType::intersection(&sparse_a, &sparse_b),
            &sparse_a & &sparse_b
        );
        let dense_a: RoaringTreemap = (0..100_000u64)
            .filter(|v| v % 3 != 0)
            .map(|v| v + (1 << 32) - 50_000)
            .collect();
        let dense_b: RoaringTreemap = (0..100_000u64)
            .filter(|v| v % 5 != 0)
            .map(|v| v + (1 << 32) - 50_000)
            .collect();
        assert_eq!(
            RoaringType::intersection(&dense_a, &dense_b),
            &dense_a & &dense_b
        );
        assert_eq!(
            RoaringType::intersection(&RoaringTreemap::new(), &dense_a),
            RoaringTreemap::new()
        );
    }

    #[test]
    fn heap_size_counts_nodes_and_containers() {
        let compacted = |vals: &[u64]| {
            let mut t = bm(vals);
            t.compact(); // one-value arrays at capacity 1
            t.heap_size()
        };
        let empty = RoaringTreemap::new().heap_size();
        assert_eq!(empty, 32); // the boxed struct
        let one = compacted(&[1]);
        let two = compacted(&[1, 1 << 32]);
        // One leaf node plus one sub-bitmap: a 40-byte record (48) and a
        // one-value array (8).
        assert_eq!(one - empty, BTREE_LEAF_BYTES + 48 + 8);
        assert_eq!(two - one, 48 + 8);
        // 30 sub-bitmaps: four leaves and one internal node; each sub-bitmap
        // was built by inserts (record vector at capacity 4: 160, array 8).
        let many: RoaringTreemap = (0..30u64).map(|h| h << 32).collect();
        assert_eq!(
            many.heap_size(),
            32 + 4 * BTREE_LEAF_BYTES + BTREE_INTERNAL_BYTES + 30 * (160 + 8)
        );
    }

    #[test]
    fn export_layout_matches_roaring_serializer() {
        // serialize_noting_ties must write exactly what roaring-rs writes.
        let mut state = 0x3C6E_F372_FE94_F82Bu64;
        for _ in 0..40 {
            let mut t: RoaringTreemap = (0..xorshift(&mut state) % 3000)
                .map(|_| ((xorshift(&mut state) % 5) << 32) | (xorshift(&mut state) % 300_000))
                .collect();
            if xorshift(&mut state).is_multiple_of(2) {
                t.insert_range((2 << 32) + 5..(2 << 32) + 8); // a tied run
                t.insert_range(9..60_000);
            }
            let mut expected = Vec::new();
            t.serialize_into(&mut expected).unwrap();
            let (buf, _) = serialize_noting_ties(&t).unwrap();
            assert_eq!(buf, expected);
        }
        let mut t = RoaringTreemap::new();
        t.insert_range(5..8);
        t.insert_range((3 << 32) + 5..(3 << 32) + 8);
        t.insert(9 << 32);
        let tie = Tie {
            index: 0,
            key: 0,
            card: 3,
        };
        assert_eq!(
            serialize_noting_ties(&t).unwrap().1,
            vec![(0, vec![tie]), (3, vec![tie])]
        );
        assert_eq!(
            serialize_noting_ties(&RoaringTreemap::new()).unwrap().0,
            0u64.to_le_bytes()
        );
    }

    #[test]
    fn deserialize_drops_empty_sub_bitmaps() {
        // {1} under high word 0, plus an empty sub-bitmap under 7.
        let mut blob = Vec::new();
        blob.extend_from_slice(&2u64.to_le_bytes());
        blob.extend_from_slice(&0u32.to_le_bytes());
        roaring::RoaringBitmap::from_iter([1u32])
            .serialize_into(&mut blob)
            .unwrap();
        blob.extend_from_slice(&7u32.to_le_bytes());
        roaring::RoaringBitmap::new()
            .serialize_into(&mut blob)
            .unwrap();
        let t = <RoaringTreemap as RoaringType>::deserialize_from(&blob[..]).unwrap();
        assert_eq!(t, bm(&[1]));
        assert!(RoaringType::is_subset(&t, &bm(&[1])));
        assert_eq!(t.bitmaps().count(), 1);
    }

    /// A 64-bit blob from (high word, values) entries, in the given order.
    fn blob_of(entries: &[(u32, &[u32])]) -> Vec<u8> {
        let mut blob = (entries.len() as u64).to_le_bytes().to_vec();
        for &(hi, values) in entries {
            blob.extend_from_slice(&hi.to_le_bytes());
            roaring::RoaringBitmap::from_iter(values.iter().copied())
                .serialize_into(&mut blob)
                .unwrap();
        }
        blob
    }

    /// CRoaring rejects high words that do not strictly increase (checked
    /// with pyroaring's BitMap64.deserialize); roaring-rs keeps the last of a
    /// repeated word, silently dropping the values under the earlier one.
    #[test]
    fn deserialize_rejects_repeated_or_decreasing_high_words() {
        let decode = |b: &[u8]| <RoaringTreemap as RoaringType>::deserialize_from(b);
        let ok = blob_of(&[(0, &[1, 2]), (3, &[5])]);
        assert_eq!(decode(&ok).unwrap(), bm(&[1, 2, (3 << 32) + 5]));
        for bad in [
            blob_of(&[(0, &[1, 2]), (0, &[7])]),
            blob_of(&[(3, &[5]), (0, &[1])]),
            blob_of(&[(0, &[1]), (2, &[]), (2, &[9])]),
        ] {
            assert!(decode(&bad).is_err());
        }
        // roaring-rs's own decoder would have accepted the repeat and lost {1, 2}.
        let repeated = blob_of(&[(0, &[1, 2]), (0, &[7])]);
        assert_eq!(
            RoaringTreemap::deserialize_from(&repeated[..]).unwrap(),
            bm(&[7])
        );
    }

    /// What 1.1.1 accepted, decoded as it decoded it (for AOF replay and
    /// replication from a 1.1.1 primary): the last of a repeated high word
    /// wins, decreasing high words are fine, trailing bytes are ignored.
    #[test]
    fn deserialize_legacy_matches_1_1_1() {
        let decode = |b: &[u8]| <RoaringTreemap as RoaringType>::deserialize_legacy(b);
        let repeated = blob_of(&[(0, &[1, 2]), (0, &[7])]);
        assert_eq!(decode(&repeated).unwrap(), bm(&[7]));
        let decreasing = blob_of(&[(5, &[1, 2]), (2, &[7])]);
        assert_eq!(
            decode(&decreasing).unwrap(),
            bm(&[(2 << 32) + 7, (5 << 32) + 1, (5 << 32) + 2])
        );
        let mut trailing = blob_of(&[(1, &[3])]);
        trailing.extend_from_slice(b"JUNK");
        assert_eq!(decode(&trailing).unwrap(), bm(&[(1 << 32) + 3]));
        // An empty last entry for a repeated word: 1.1.1 held nothing there.
        let emptied = blob_of(&[(0, &[1]), (0, &[])]);
        let t = decode(&emptied).unwrap();
        assert!(t.is_empty() && t.bitmaps().count() == 0);
        // Strictly valid blobs decode the same either way.
        let ok = blob_of(&[(0, &[1, 2]), (3, &[5])]);
        assert_eq!(
            decode(&ok).unwrap(),
            <RoaringTreemap as RoaringType>::deserialize_from(&ok[..]).unwrap()
        );
        assert!(
            decode(&repeated[..repeated.len() - 1]).is_err(),
            "truncated"
        );
    }

    #[test]
    fn decode_exact_rejects_trailing_bytes() {
        use crate::bitmap_type::decode_exact;
        let blob = blob_of(&[(0, &[1, 2]), (3, &[5])]);
        assert_eq!(
            decode_exact::<RoaringTreemap>(&blob).unwrap(),
            bm(&[1, 2, (3 << 32) + 5])
        );
        for extra in [&b"\0"[..], b"\x01\x02", b"junk"] {
            let mut padded = blob.clone();
            padded.extend_from_slice(extra);
            assert!(decode_exact::<RoaringTreemap>(&padded).is_err());
        }
        // Truncated anywhere: an error, never a partial set.
        for cut in 0..blob.len() {
            assert!(
                decode_exact::<RoaringTreemap>(&blob[..cut]).is_err(),
                "cut {cut}"
            );
        }
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
    fn remove_many_counts_duplicates_once() {
        let mut b = bm(&[100]);
        assert_eq!(b.remove_many(&[100, 100, 100]), 1);
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
        assert_eq!(RoaringType::insert_many(&mut b, &[5_000_000_000, 1, 2]), 3);
        assert_eq!(RoaringType::min_val(&b), Some(1));
        assert_eq!(RoaringType::max_val(&b), Some(5_000_000_000));
        assert!(RoaringType::contains(&b, 1) && !RoaringType::contains(&b, 4));
        assert_eq!(RoaringType::remove_many(&mut b, &[1, 9]), 1);
        assert!(!RoaringType::contains(&b, 1));
        assert_eq!(RoaringType::insert_range_exclusive(&mut b, 10, 13), 3);
        assert_eq!(
            RoaringType::iter_values(&b).count() as u64,
            RoaringType::len(&b)
        );

        let other = bm(&[2, 100]);
        assert!(!RoaringType::is_disjoint(&b, &other));
        assert!(RoaringType::is_subset(&bm(&[2]), &other));
        assert_eq!(RoaringType::intersection_len(&b, &other), 1);
        assert_eq!(RoaringType::union(&b, &other), &b | &other);
        assert_eq!(RoaringType::intersection(&b, &other), &b & &other);
        assert_eq!(RoaringType::symmetric_difference(&b, &other), &b ^ &other);
        assert_eq!(RoaringType::difference(&bm(&[1, 2]), &bm(&[2])), bm(&[1]));
        let mut u = bm(&[1]);
        RoaringType::bitor_assign_owned(&mut u, bm(&[2]));
        assert_eq!(u, bm(&[1, 2]));

        let mut c = bm(&[1, 2]);
        RoaringType::clear(&mut c);
        assert!(c.is_empty());
    }

    #[test]
    fn stat_matches_upstream_layout_and_units() {
        // Aggregated across sub-bitmaps: the two values land in different
        // 32-bit partitions, so two array containers of one value each.
        let b = bm(&[1, 5_000_000_000]);
        assert_eq!(
            b.stat_fields().text(),
            "type: bitmap64\ncardinality: 2\nnumber of containers: 2\n\
             max value: 5000000000\nmin value: 1\nnumber of array containers: 2\n\
             \tarray container values: 2\n\tarray container bytes: 4\n\
             bitset  containers: 0\n\tbitset  container values: 0\n\
             \tbitset  container bytes: 0\nrun containers: 0\n\
             \trun container values: 0\n\trun container bytes: 0\n"
        );
        assert!(b.stat_fields().json().starts_with(
            "{\"type\":\"bitmap64\",\"cardinality\":\"2\",\"number_of_containers\":\"2\",\
             \"max_value\":\"5000000000\",\"min_value\":\"1\",\"array_container\":{"
        ));

        let e = RoaringTreemap::new().stat_fields();
        assert_eq!(
            (e.cardinality, e.containers, e.max, e.min),
            (0, 0, 0, u64::MAX)
        );

        // A full 32-bit sub-bitmap: run values counted in 64 bits.
        let mut f = RoaringTreemap::new();
        f.insert_range(1u64 << 32..2u64 << 32);
        f.optimize();
        let f = f.stat_fields();
        assert_eq!(
            (f.runs, f.run_values, f.run_bytes),
            (65536, 1 << 32, 393216)
        );
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
