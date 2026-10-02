//! valkey-roaring: RoaringType implementation for RoaringBitmap (u32).

use crate::bitmap_type::{RoaringType, StatFields};
use crate::canonical::{self, Tie};
use crate::parse;
use roaring::RoaringBitmap;
use std::io;
use std::mem::size_of;

/// Size of one roaring-rs container record (u16 key plus store enum) in a
/// bitmap's container vector. Private to roaring-rs, so measured: a 0.11.5
/// bitmap of N containers allocates N * 40 bytes for that vector.
pub(crate) const CONTAINER_RECORD_BYTES: usize = 40;
/// One bitset container's word array.
const BITSET_BYTES: usize = 8192;
/// Average values per container from which `intersection` copies and
/// filters rather than building its result fresh (measured crossover).
pub(crate) const INTERSECT_IN_PLACE_FROM: u64 = 32;

/// Bytes the allocator hands out for a request: Valkey's used_memory counts
/// jemalloc's usable size, i.e. the request rounded up to a size class (8,
/// then steps of 16 to 128, then four classes per power of two).
pub(crate) fn alloc_size(bytes: usize) -> usize {
    if bytes <= 8 {
        return 8;
    }
    if bytes <= 128 {
        return bytes.next_multiple_of(16);
    }
    let step = 1 << ((usize::BITS - (bytes - 1).leading_zeros() - 1) - 2);
    bytes.next_multiple_of(step)
}

/// Capacity of a vector of `n` elements grown by pushes or inserts: Rust
/// starts it at 4 and doubles.
fn grown_capacity(n: usize) -> usize {
    n.max(4).next_power_of_two()
}

/// Estimated heap bytes of a bitmap's containers, excluding the struct
/// itself (callers add it where it is boxed on its own), the part of it a
/// compacting copy would reclaim, and the container count.
///
/// Every container is a record in one vector, plus a payload allocation for
/// array (u16 values), run (4-byte intervals) and bitset (8 KiB) stores.
/// Vector capacities are not exposed, so they are estimated from history:
/// copied or decoded bitmaps hold exact capacities everywhere, bitmaps built
/// by inserts hold Rust's doubling growth. The array statistics tell them
/// apart (capacity equal to the values held means exact); a bitmap without
/// arrays gets exact sizes with the minimum vector capacity of 4, which is
/// what a run container written by a range insert holds.
///
/// roaring-rs statistics report byte counts in mixed units: array containers
/// as capacity * 4 (it sizes elements as u32; they are u16), bitset containers
/// as their bit count, run containers as their serialized size (a 2-byte run
/// count plus 4 bytes per run). The unit test `heap_model_units` pins these.
pub(crate) fn containers_heap_size(bm: &RoaringBitmap) -> (usize, usize, usize) {
    let s = bm.statistics();
    let n = s.n_containers as usize;
    if n == 0 {
        return (0, 0, 0);
    }
    let (n_array, n_run) = (s.n_array_containers as usize, s.n_run_containers as usize);
    let array_capacity = s.n_bytes_array_containers as usize / 4; // u16 slots
    let array_values = s.n_values_array_containers as usize;
    let runs = (s.n_bytes_run_containers as usize).saturating_sub(2 * n_run) / 4;
    let exact = n_array > 0 && array_capacity == array_values;
    let grown = n_array > 0 && array_capacity > array_values;

    let records = |capacity: usize| alloc_size(capacity * CONTAINER_RECORD_BYTES);
    // Payloads by the average size per container of each kind.
    let arrays = |slots: usize| match n_array {
        0 => 0,
        _ => n_array * alloc_size(2 * slots.div_ceil(n_array)),
    };
    let run_payload = |per_container: usize| n_run * alloc_size(4 * per_container);
    let runs_each = runs.div_ceil(n_run.max(1));
    let bitsets = s.n_bitset_containers as usize * BITSET_BYTES;

    let record_capacity = if exact {
        n
    } else if grown {
        grown_capacity(n)
    } else {
        n.max(4)
    };
    // A single-run container comes from vec![interval] (capacity 1).
    let run_capacity = if exact || runs_each <= 1 {
        runs_each
    } else {
        grown_capacity(runs_each)
    };
    let heap =
        records(record_capacity) + arrays(array_capacity) + run_payload(run_capacity) + bitsets;
    let compacted = records(n) + arrays(array_values) + run_payload(runs_each) + bitsets;
    (heap, heap.saturating_sub(compacted), n)
}

/// roaring-rs's in-place `|=` binary-searches the left side for each
/// container on the right and inserts each one the left side lacks into its
/// container vector, shifting everything after it: cheap for a few,
/// quadratic for many (32,768 interleaved containers: 190 ms). The
/// by-reference `|` merges in one pass (1.5 ms there) but copies every
/// container of both sides, an allocation each, which costs about as much
/// as shifting a container record SHIFTS_PER_COPY times.
const SHIFTS_PER_COPY: usize = 20;
/// Up to this many containers on the right, in place is the cheaper one
/// whatever the sizes (it stays within the copy's cost even on a
/// million-container left side).
pub(crate) const UNION_IN_PLACE_FREE: usize = 16;
/// Record shifts that always count as cheap: memmoves of short container
/// vectors (32,768 shifts move about 1.3 MB). Small unions stay in place,
/// where the owned form also moves the right side's containers instead of
/// copying them.
const UNION_CHEAP_SHIFTS: usize = 1 << 15;
/// Counting the containers the left side lacks costs about 70 ns per
/// container on the right (a range lookup and an iterator skip; more on
/// cold data), as much as a whole union of sparse sets, so it is paid only
/// for a right side at most 1/PROBE_SIZE_RATIO of the left: a delta into a
/// big key, where the one-pass union would copy the whole key. Between
/// sides of comparable size, both strategies cost about the same when most
/// containers are shared and the one-pass union wins when few are, so it
/// is taken without counting.
const PROBE_SIZE_RATIO: usize = 8;

/// Whether `bm |= other` in place costs less than the one-pass union: the
/// record shifts of inserting the missing containers into `bm`'s vector,
/// about absent * (n_bm + absent / 2), against copying n_bm + n_other
/// containers.
pub(crate) fn union_in_place_is_cheap(bm: &RoaringBitmap, other: &RoaringBitmap) -> bool {
    let n_other = other.statistics().n_containers as usize;
    if n_other <= UNION_IN_PLACE_FREE {
        return true;
    }
    let n_bm = bm.statistics().n_containers as usize; // ~0.6 ns per container
    let affordable = |absent: usize| {
        let shifts = absent * (n_bm + absent / 2);
        shifts <= UNION_CHEAP_SHIFTS || shifts <= SHIFTS_PER_COPY * (n_bm + n_other)
    };
    // Cheap even if every container were missing: small sets.
    if affordable(n_other) {
        return true;
    }
    if n_other * PROBE_SIZE_RATIO > n_bm {
        return false;
    }
    // A small delta into a big bitmap: visit each container of `other` by
    // its first value and count the keys `bm` has no container for,
    // stopping once in place is out of budget.
    let mut absent = 0usize;
    let mut it = other.iter();
    while let Some(v) = it.next() {
        let lo = v & !0xFFFF;
        if bm.range(lo..=lo | 0xFFFF).next().is_none() {
            absent += 1;
            if absent > UNION_IN_PLACE_FREE && !affordable(absent) {
                return false;
            }
        }
        match lo.checked_add(1 << 16) {
            Some(next) => it.advance_to(next),
            None => break,
        }
    }
    true
}

/// Builds a bitmap from sorted, deduplicated values with every container at
/// its exact size, by writing the portable layout and decoding it. roaring-rs
/// offers no way to size a container: `from_sorted_iter` pushes value by
/// value into vectors that grow by doubling, so SETINTARRAY also needed a
/// compacting copy. This builds about 3x faster than `from_sorted_iter`
/// alone (1M values: 0.8 against 2.5 ms; 10k clustered: 10 against 32 us),
/// with the same encodings: arrays up to 4,096 values, bitsets above.
pub(crate) fn from_sorted_exact(vals: &[u32]) -> RoaringBitmap {
    const NO_RUNS: u32 = 12346; // portable cookie without run containers
                                // (key, start, end) of each high-16 group.
    let mut groups: Vec<(u16, usize, usize)> = Vec::new();
    let mut start = 0;
    while start < vals.len() {
        let key = vals[start] >> 16;
        let len = vals[start..].partition_point(|&v| v >> 16 == key);
        groups.push((key as u16, start, start + len));
        start += len;
    }
    let bytes = |s: usize, e: usize| if e - s > 4096 { 8192 } else { 2 * (e - s) };
    let header = 8 + 8 * groups.len();
    let size = header + groups.iter().map(|&(_, s, e)| bytes(s, e)).sum::<usize>();
    let mut blob = Vec::with_capacity(size);
    blob.extend_from_slice(&NO_RUNS.to_le_bytes());
    blob.extend_from_slice(&(groups.len() as u32).to_le_bytes());
    for &(key, s, e) in &groups {
        blob.extend_from_slice(&key.to_le_bytes());
        blob.extend_from_slice(&((e - s - 1) as u16).to_le_bytes());
    }
    let mut offset = header;
    for &(_, s, e) in &groups {
        blob.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += bytes(s, e);
    }
    for &(_, s, e) in &groups {
        if e - s > 4096 {
            let mut words = [0u64; 1024];
            for &v in &vals[s..e] {
                let low = (v & 0xFFFF) as usize;
                words[low >> 6] |= 1 << (low & 63);
            }
            for w in words {
                blob.extend_from_slice(&w.to_le_bytes());
            }
        } else {
            for &v in &vals[s..e] {
                blob.extend_from_slice(&(v as u16).to_le_bytes());
            }
        }
    }
    debug_assert_eq!(blob.len(), size);
    // The layout is valid by construction, so the decoder's checks are
    // skipped.
    RoaringBitmap::deserialize_unchecked_from(&blob[..]).expect("valid by construction")
}

/// The complement of `src` within [0, last], for a `src` with no value above
/// `last` (BITOP NOT), built directly in the portable layout and decoded
/// once. roaring-rs's route, a full-range bitmap XORed with the source,
/// splits a full run container once per value of each array container (14x
/// slower than upstream on 1M sparse values). Here the source is walked by
/// runs of consecutive values, each chunk's gaps become its container in
/// the smallest encoding (run, array or bitset), and a chunk the source
/// lacks is a single run.
///
/// NOT WIRED IN (round 4, paused): measured against the XOR route it is
/// 20x faster on clustered sources (1M values: 0.33 against 7.2 ms) but much
/// slower on dense ones (bitset sources walked run by run: 1M dense values
/// 8.0 against 0.12 ms) and on 1M sparse values (28.6 against 18.5 ms).
/// Kept, with its equivalence test, for a per-chunk hybrid.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn complement_within(src: &RoaringBitmap, last: u32) -> RoaringBitmap {
    const COOKIE_RUNS: u32 = 12347; // portable cookie with run containers
    const ARRAY: u8 = 0;
    const BITSET: u8 = 1;
    const RUN: u8 = 2;
    debug_assert!(src.max().is_none_or(|m| m <= last));
    let last_key = last >> 16;
    // Per result container: key, kind and cardinality; payloads in order.
    let mut descs: Vec<(u16, u8, u32)> = Vec::with_capacity(last_key as usize + 1);
    let mut payload: Vec<u8> = Vec::new();
    let mut gaps: Vec<(u32, u32)> = Vec::new();
    let mut ranges = src.iter();
    let mut pending = ranges.next_range().map(|r| (*r.start(), *r.end()));
    for key in 0..=last_key {
        let lo = key << 16;
        let hi = (lo | 0xFFFF).min(last);
        // Gaps between the source's runs inside [lo, hi]. `cur` is the
        // first value not yet classified; u64 so hi + 1 cannot overflow.
        gaps.clear();
        let mut cur = u64::from(lo);
        while let Some((s, e)) = pending {
            if s > hi {
                break;
            }
            if u64::from(s) > cur {
                gaps.push((cur as u32, s - 1));
            }
            cur = u64::from(e.min(hi)) + 1;
            if e > hi {
                pending = Some((hi + 1, e)); // runs on into the next chunk
                break;
            }
            pending = ranges.next_range().map(|r| (*r.start(), *r.end()));
        }
        if cur <= u64::from(hi) {
            gaps.push((cur as u32, hi));
        }
        let card: u32 = gaps.iter().map(|&(a, b)| b - a + 1).sum();
        if card == 0 {
            continue;
        }
        let run_bytes = 2 + 4 * gaps.len();
        let kind = if card <= 4096 {
            if run_bytes < 2 * card as usize {
                RUN
            } else {
                ARRAY
            }
        } else if run_bytes < 8192 {
            RUN
        } else {
            BITSET
        };
        match kind {
            RUN => {
                payload.extend_from_slice(&(gaps.len() as u16).to_le_bytes());
                for &(a, b) in &gaps {
                    payload.extend_from_slice(&(a as u16).to_le_bytes());
                    payload.extend_from_slice(&((b - a) as u16).to_le_bytes());
                }
            }
            ARRAY => {
                for &(a, b) in &gaps {
                    for v in a..=b {
                        payload.extend_from_slice(&(v as u16).to_le_bytes());
                    }
                }
            }
            _ => {
                let mut words = [0u64; 1024];
                for &(a, b) in &gaps {
                    let (a, b) = ((a & 0xFFFF) as usize, (b & 0xFFFF) as usize);
                    let (wa, wb) = (a >> 6, b >> 6);
                    let first = !0u64 << (a & 63);
                    let last_mask = !0u64 >> (63 - (b & 63));
                    if wa == wb {
                        words[wa] |= first & last_mask;
                    } else {
                        words[wa] |= first;
                        words[wa + 1..wb].fill(!0);
                        words[wb] |= last_mask;
                    }
                }
                for w in words {
                    payload.extend_from_slice(&w.to_le_bytes());
                }
            }
        }
        descs.push((key as u16, kind, card));
    }
    if descs.is_empty() {
        return RoaringBitmap::new();
    }
    // Header: cookie with the container count, run flags, descriptions,
    // and offsets from four containers up.
    let n = descs.len();
    let with_offsets = n >= 4;
    let header = 4 + n.div_ceil(8) + 4 * n + if with_offsets { 4 * n } else { 0 };
    let mut blob = Vec::with_capacity(header + payload.len());
    blob.extend_from_slice(&(COOKIE_RUNS | ((n as u32 - 1) << 16)).to_le_bytes());
    let mut flags = vec![0u8; n.div_ceil(8)];
    for (i, &(_, kind, _)) in descs.iter().enumerate() {
        if kind == RUN {
            flags[i / 8] |= 1 << (i % 8);
        }
    }
    blob.extend_from_slice(&flags);
    for &(key, _, card) in &descs {
        blob.extend_from_slice(&key.to_le_bytes());
        blob.extend_from_slice(&((card - 1) as u16).to_le_bytes());
    }
    if with_offsets {
        let mut offset = header;
        for &(_, kind, card) in &descs {
            blob.extend_from_slice(&(offset as u32).to_le_bytes());
            offset += match kind {
                ARRAY => 2 * card as usize,
                BITSET => 8192,
                _ => {
                    let runs = u16::from_le_bytes([
                        payload[offset - header],
                        payload[offset - header + 1],
                    ]);
                    2 + 4 * runs as usize
                }
            };
        }
    }
    blob.extend_from_slice(&payload);
    RoaringBitmap::deserialize_unchecked_from(&blob[..]).expect("valid by construction")
}

/// Set equality without roaring-rs's `==` on two bitset containers, which
/// walks every set bit (19-116x slower than upstream on dense keys). `==`
/// stays the check when `a` holds no bitset: it compares container counts
/// first and arrays and runs as slices, far faster than a subset test (1M
/// sparse values: 34 us against 976 us), and a bitset in `b` can then only
/// meet an array (unequal at once) or a run (compared by interval). With a
/// bitset in `a`, equal cardinality and container count plus `is_subset`
/// (bitsets a word at a time) decide it. One statistics pass on `a` picks
/// the way (~0.6 ns per container).
pub(crate) fn bitmaps_equal(a: &RoaringBitmap, b: &RoaringBitmap) -> bool {
    let sa = a.statistics();
    if sa.n_bitset_containers == 0 {
        return a == b;
    }
    let sb = b.statistics();
    sa.cardinality == sb.cardinality && sa.n_containers == sb.n_containers && a.is_subset(b)
}

/// `bm |= other`, in time linear in both sizes.
pub(crate) fn union_into(bm: &mut RoaringBitmap, other: &RoaringBitmap) {
    if union_in_place_is_cheap(bm, other) {
        *bm |= other;
    } else {
        *bm = &*bm | other;
    }
}

/// The values of the tied containers, as a bitmap of array containers (a
/// new container starts out as an array, and a tie never holds more than
/// 4096 values). One iterator walks the bitmap; it only binary-searches to
/// a tie that does not directly follow the previous one, and takes exactly
/// `card` values from each.
pub(crate) fn tied_values(bm: &RoaringBitmap, ties: &[Tie]) -> RoaringBitmap {
    let mut vals = Vec::with_capacity(ties.iter().map(|t| t.card).sum());
    let mut it = bm.iter();
    let mut at = 0; // index of the container `it` stands at the start of
    for tie in ties {
        if tie.index != at {
            it.advance_to(u32::from(tie.key) << 16);
        }
        vals.extend(it.by_ref().take(tie.card));
        at = tie.index + 1;
    }
    RoaringBitmap::from_sorted_iter(vals).expect("ascending values")
}

/// Re-encode the tied containers as arrays. Their values are a subset of
/// the bitmap, so `^= &tied` drops exactly those containers (x ^ x is
/// empty), and `^= tied` then moves the arrays in: the keys no longer
/// overlap, so that symmetric difference is a union. In roaring-rs 0.11.5
/// both are one merge over the two container sequences that moves, not
/// copies, the untouched containers. (Removing and re-inserting containers
/// one at a time shifted the container vector per tie: 3 s for 65,536.)
fn demote_to_arrays(bm: &mut RoaringBitmap, ties: &[Tie]) {
    let tied = tied_values(bm, ties);
    if tied.len() == bm.len() {
        // Every container was tied: the arrays are the whole bitmap.
        *bm = tied;
        return;
    }
    *bm ^= &tied;
    *bm ^= tied;
}

/// Least estimated slack per container worth a compacting copy, which costs
/// about one allocation per container.
const MIN_SLACK_PER_CONTAINER: usize = 64;

/// Shared policy for `RoaringType::trim`: a copy pays off once the estimated
/// slack is at least an eighth of the footprint and averages at least
/// MIN_SLACK_PER_CONTAINER bytes per container. (The floor keeps results
/// made of many near-exact containers, such as a NOT's hundreds of
/// single-run containers, from being copied for a few bytes each: the run
/// estimate works from averages and can see a few bytes of slack in every
/// container where only a handful have any.)
pub(crate) fn worth_compacting(heap: usize, slack: usize, containers: usize) -> bool {
    slack * 8 >= heap && slack >= MIN_SLACK_PER_CONTAINER * containers
}

impl RoaringType for RoaringBitmap {
    type Value = u32;

    const MAX_VALUE: u32 = u32::MAX;
    const VALUE_DESCRIPTION: &'static str = "must be an unsigned 32 bit integer";
    const ERR_END_BEFORE_START: &'static str = "ERR invalid end: must be >= start";
    const GETBIT_PARSES_BEFORE_KEY_CHECK: bool = false;

    fn parse_value_arg(bytes: &[u8]) -> Option<u32> {
        parse::parse_u32_strict(bytes)
    }

    fn value_to_i64(v: u32) -> i64 {
        v as i64
    }

    fn value_to_u64(v: u32) -> u64 {
        u64::from(v)
    }

    fn new() -> Self {
        RoaringBitmap::new()
    }

    fn full() -> Self {
        RoaringBitmap::full()
    }

    fn from_values(mut vals: Vec<u32>) -> Self {
        // Already-sorted input (the common case) sorts in linear time.
        vals.sort_unstable();
        vals.dedup();
        from_sorted_exact(&vals)
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

    fn insert_many(&mut self, vals: &[u32]) -> usize {
        vals.iter().filter(|&&v| self.insert(v)).count()
    }

    fn remove_many(&mut self, vals: &[u32]) -> usize {
        vals.iter().filter(|&&v| self.remove(v)).count()
    }

    fn contains_range_exclusive(&self, start: u32, end: u32) -> bool {
        self.contains_range(start..end)
    }

    fn len(&self) -> u64 {
        RoaringBitmap::len(self)
    }

    fn container_count(&self) -> usize {
        self.statistics().n_containers as usize
    }

    fn min_val(&self) -> Option<u32> {
        self.min()
    }

    fn max_val(&self) -> Option<u32> {
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
        // In place (moving containers, no copies) unless that would insert
        // many containers one at a time (see union_in_place_is_cheap).
        if union_in_place_is_cheap(self, &other) {
            *self |= other;
        } else {
            *self = &*self | &other;
        }
    }

    fn union(&self, other: &Self) -> Self {
        self | other
    }

    fn intersection(&self, other: &Self) -> Self {
        // Sparse containers: build the result fresh. From ~32 values per
        // container up, copying and filtering in place measures faster
        // (15-20% at 1M values; roaring-rs 0.11.5).
        if RoaringBitmap::len(self) < INTERSECT_IN_PLACE_FROM * self.container_count() as u64 {
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
        // Copy-then-subtract measures faster than `self - other` for
        // anything beyond small sets (roaring-rs 0.11.5).
        let mut out = self.clone();
        out -= other;
        out
    }

    fn is_disjoint(&self, other: &Self) -> bool {
        RoaringBitmap::is_disjoint(self, other)
    }

    fn is_subset(&self, other: &Self) -> bool {
        RoaringBitmap::is_subset(self, other)
    }

    fn set_eq(&self, other: &Self) -> bool {
        bitmaps_equal(self, other)
    }

    fn intersection_len(&self, other: &Self) -> u64 {
        RoaringBitmap::intersection_len(self, other)
    }

    fn select(&self, n: u64) -> Option<u32> {
        if n > u32::MAX as u64 {
            return None;
        }
        RoaringBitmap::select(self, n as u32)
    }

    fn nth_absent(&self, n: u64) -> Option<u32> {
        // Find the nth element NOT present in the set (1-indexed).
        // Gap-skipping walk over runs of consecutive values (O(runs), where
        // upstream v1.7.4 walks values): `candidate` is the smallest value
        // not yet classified.
        if n == 0 {
            return None;
        }
        let mut n = n;
        let mut candidate: u64 = 0;
        let mut it = self.iter();
        while let Some(run) = it.next_range() {
            let (start, end) = (*run.start() as u64, *run.end() as u64);
            if start > candidate {
                let gap = start - candidate;
                if n <= gap {
                    return Some((candidate + n - 1) as u32);
                }
                n -= gap;
            }
            candidate = end + 1;
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
        range_bm ^= self;
        range_bm
    }

    fn serialize_into<W: io::Write>(&self, writer: W) -> io::Result<()> {
        RoaringBitmap::serialize_into(self, writer)
    }

    fn deserialize_from<R: io::Read>(reader: R) -> io::Result<Self> {
        RoaringBitmap::deserialize_from(reader)
    }

    fn deserialize_legacy(bytes: &[u8]) -> io::Result<Self> {
        // Stops at the end of the blob, whatever follows.
        RoaringBitmap::deserialize_from(bytes)
    }

    fn serialized_size(&self) -> usize {
        RoaringBitmap::serialized_size(self)
    }

    fn export_canonical(&mut self) -> io::Result<Vec<u8>> {
        RoaringBitmap::optimize(self);
        let mut buf = Vec::with_capacity(RoaringBitmap::serialized_size(self));
        RoaringBitmap::serialize_into(self, &mut buf)?;
        if canonical::has_no_runs(&buf) {
            return Ok(buf);
        }
        let tied = match canonical::tied_run_containers(&buf) {
            Some(tied) if !tied.is_empty() => tied,
            _ => return Ok(buf),
        };
        demote_to_arrays(self, &tied);
        buf.clear();
        RoaringBitmap::serialize_into(self, &mut buf)?;
        Ok(buf)
    }

    fn optimize(&mut self) -> bool {
        RoaringBitmap::optimize(self)
    }

    fn heap_size(&self) -> usize {
        alloc_size(size_of::<RoaringBitmap>()) + containers_heap_size(self).0
    }

    fn trim(&mut self) {
        let (heap, slack, containers) = containers_heap_size(self);
        if worth_compacting(heap, slack, containers) {
            self.compact();
        }
    }

    fn insert_range_exclusive(&mut self, start: u32, end: u32) -> u64 {
        self.insert_range(start..end)
    }

    type Values<'a> = roaring::bitmap::Iter<'a>;

    fn iter_values(&self) -> Self::Values<'_> {
        self.iter()
    }

    fn iter_from(&self, start: u32) -> Self::Values<'_> {
        self.range(start..)
    }

    fn from_bit_array(bits: &[u8]) -> Self {
        // Positions come out ascending: built at exact size in one go.
        let ones: Vec<u32> = bits
            .iter()
            .enumerate()
            .filter(|&(_, &b)| b == b'1')
            .map(|(i, _)| i as u32)
            .collect();
        from_sorted_exact(&ones)
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

    fn stat_fields(&self) -> StatFields {
        // CRoaring's 32-bit statistics count in uint32_t, so every field but
        // the cardinality wraps the way upstream's does (a full bitmap's run
        // values read 0).
        let s = self.statistics();
        let cardinality = s.cardinality;
        let w = |v: u64| u64::from(v as u32);
        let array_values = u64::from(s.n_values_array_containers);
        let bitset_values = s.n_values_bitset_containers;
        StatFields {
            kind: "bitmap",
            cardinality,
            containers: w(s.n_containers.into()),
            max: s.max_value.map_or(0, u64::from),
            min: s.min_value.map_or(u64::from(u32::MAX), u64::from),
            arrays: w(s.n_array_containers.into()),
            array_values: w(array_values),
            array_bytes: w(2 * array_values),
            bitsets: w(s.n_bitset_containers.into()),
            bitset_values: w(bitset_values),
            bitset_bytes: w(BITSET_BYTES as u64 * u64::from(s.n_bitset_containers)),
            runs: w(s.n_run_containers.into()),
            run_values: w(cardinality - array_values - bitset_values),
            run_bytes: w(s.n_bytes_run_containers),
        }
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
    fn remove_many_counts_duplicates_once() {
        // Upstream v1.7.4 fix: duplicate offsets are counted once.
        let mut b = bm(&[5, 7]);
        assert_eq!(b.remove_many(&[5, 5, 5]), 1);
        assert_eq!(b, bm(&[7]));
        assert_eq!(b.remove_many(&[9]), 0);
        assert_eq!(b.remove_many(&[7, 7]), 1);
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
    fn from_values_sorts_and_dedups() {
        let b = RoaringBitmap::from_values(vec![9, 1, 70_000, 1, 5, 9, 70_000]);
        assert_eq!(b, bm(&[1, 5, 9, 70_000]));
        assert_eq!(RoaringBitmap::from_values(vec![]), RoaringBitmap::new());
        assert_eq!(
            RoaringBitmap::from_values(vec![u32::MAX, 0]),
            bm(&[0, u32::MAX])
        );
        // Agrees with insert-one-by-one on random input with duplicates.
        let mut state = 0x51_7CC1_B727_220Au64;
        for _ in 0..50 {
            let vals: Vec<u32> = (0..xorshift(&mut state) % 3000)
                .map(|_| (xorshift(&mut state) % 300_000) as u32)
                .collect();
            assert_eq!(RoaringBitmap::from_values(vals.clone()), bm(&vals));
        }
    }

    #[test]
    fn iter_from_starts_at_first_value_at_or_above() {
        let b = bm(&[3, 10, 70_000, u32::MAX]);
        let from = |v| RoaringType::iter_from(&b, v).collect::<Vec<_>>();
        assert_eq!(from(0), vec![3, 10, 70_000, u32::MAX]);
        assert_eq!(from(10), vec![10, 70_000, u32::MAX]);
        assert_eq!(from(11), vec![70_000, u32::MAX]);
        assert_eq!(from(u32::MAX), vec![u32::MAX]);
        assert!(RoaringType::iter_from(&RoaringBitmap::new(), 0)
            .next()
            .is_none());
    }

    #[test]
    fn nth_absent_skips_long_runs() {
        // A dense prefix, run-encoded or not, and a run that crosses a
        // container border (65_535 -> 65_536).
        let mut dense: RoaringBitmap = (0..1_000_000u32).collect();
        assert_eq!(dense.nth_absent(1), Some(1_000_000));
        dense.optimize();
        assert_eq!(dense.nth_absent(1), Some(1_000_000));
        assert_eq!(dense.nth_absent(5), Some(1_000_004));
        let mut b = RoaringBitmap::new();
        b.insert_range(65_530..65_540);
        b.insert(65_545);
        assert_eq!(b.nth_absent(65_530), Some(65_529));
        assert_eq!(b.nth_absent(65_531), Some(65_540));
        assert_eq!(b.nth_absent(65_536), Some(65_546));
        let mut top = RoaringBitmap::new();
        top.insert_range(u32::MAX - 5..=u32::MAX);
        assert_eq!(top.nth_absent(u32::MAX as u64 - 5), Some(u32::MAX - 6));
        assert_eq!(top.nth_absent(u32::MAX as u64 - 4), None);
    }

    #[test]
    fn intersection_matches_operator_on_both_strategies() {
        // One value per container (fresh build) and dense containers (copy
        // and filter in place).
        let sparse_a: RoaringBitmap = (0..100u32).map(|i| i << 16).collect();
        let sparse_b: RoaringBitmap = (0..100u32)
            .filter(|i| i % 3 == 0)
            .map(|i| i << 16)
            .collect();
        assert_eq!(
            RoaringType::intersection(&sparse_a, &sparse_b),
            &sparse_a & &sparse_b
        );
        let dense_a: RoaringBitmap = (0..100_000u32).filter(|v| v % 3 != 0).collect();
        let dense_b: RoaringBitmap = (0..100_000u32).filter(|v| v % 5 != 0).collect();
        assert_eq!(
            RoaringType::intersection(&dense_a, &dense_b),
            &dense_a & &dense_b
        );
        assert_eq!(
            RoaringType::intersection(&RoaringBitmap::new(), &dense_a),
            RoaringBitmap::new()
        );
        assert_eq!(
            RoaringType::intersection(&dense_a, &RoaringBitmap::new()),
            RoaringBitmap::new()
        );
    }

    #[test]
    fn heap_model_units() {
        // containers_heap_size relies on roaring-rs's statistics units: array
        // bytes = capacity * 4, bitset bytes = 65_536 (bits), run bytes =
        // 2 + 4 per run. If a roaring-rs upgrade changes them, this fails and
        // the model needs adjusting.
        let mut array: RoaringBitmap = (0..100u32).collect();
        array.compact(); // capacity == len
        assert_eq!(array.statistics().n_bytes_array_containers, 400);
        let bitset: RoaringBitmap = (0..10_000u32).map(|v| v * 2).collect();
        assert_eq!(bitset.statistics().n_bytes_bitset_containers, 65_536);
        let mut run = RoaringBitmap::new();
        run.insert_range(0..100);
        run.insert_range(200..300);
        assert_eq!(run.statistics().n_bytes_run_containers, 2 + 4 * 2);

        // The boxed struct (24 -> 32 bytes) plus, per bitmap: the record
        // vector and each payload, at estimated capacity, in size classes.
        let base = 32;
        assert_eq!(RoaringBitmap::new().heap_size(), base);
        // Copied: exact capacities. One record (40 -> 48), 100 u16 (200 -> 224).
        assert_eq!(array.heap_size(), base + 48 + 224);
        assert_eq!(containers_heap_size(&array).1, 0, "nothing to reclaim");
        // No arrays to tell: record vector at its minimum capacity of 4.
        assert_eq!(bitset.heap_size(), base + 160 + BITSET_BYTES);
        // Two runs pushed into a run vector: capacity 4 (16 bytes).
        assert_eq!(run.heap_size(), base + 160 + 16);
        // Built by inserts: 1000 values per container, room for 1024.
        let grown: RoaringBitmap = (0..3u32)
            .flat_map(|c| (0..1000u32).map(move |i| (c << 16) | (i * 5)))
            .collect();
        let (heap, slack, _) = containers_heap_size(&grown);
        assert_eq!(heap, 160 + 3 * 2048);
        assert_eq!(
            slack,
            heap - (128 + 3 * 2048),
            "records 3x40 -> 128, arrays 2000 -> 2048"
        );
    }

    #[test]
    fn trim_leaves_near_exact_containers_alone() {
        // A NOT result: hundreds of full single-run containers plus a few
        // holding many runs. The run estimate works from the average, so it
        // sees a few bytes of slack in every container; copying all of them
        // for that cost a NOT on a 2,000-value key about 10 us.
        let src: RoaringBitmap = (0..15u32)
            .flat_map(|c| (0..40u32).map(move |i| c * 3_000_000 + i * 7))
            .collect();
        let flipped = src.flip_inclusive(src.max().unwrap());
        let (heap, slack, n) = containers_heap_size(&flipped);
        assert!(n > 600, "{n} containers");
        assert!(slack * 8 >= heap, "the ratio alone would compact");
        assert!(
            !worth_compacting(heap, slack, n),
            "heap {heap} slack {slack} n {n}"
        );
        // Real slack (arrays at twice their length) still compacts.
        assert!(worth_compacting(10 * 4096, 10 * 2048, 10));
    }

    #[test]
    fn alloc_size_follows_jemalloc_size_classes() {
        let cases = [
            (0, 8),
            (8, 8),
            (9, 16),
            (17, 32),
            (100, 112),
            (128, 128),
            (129, 160),
            (200, 224),
            (1025, 1280),
            (4096, 4096),
            (4097, 5120),
            (9000, 10240),
            (16_385, 20_480),
        ];
        for (request, class) in cases {
            assert_eq!(alloc_size(request), class, "request {request}");
        }
    }

    #[test]
    fn compact_preserves_the_set_and_drops_slack() {
        // Inserting one by one grows vectors by doubling: 1_000 values per
        // container end up with room for 1_024.
        let mut b = RoaringBitmap::new();
        for c in 0..50u32 {
            for v in 0..1_000u32 {
                b.insert((c << 16) | (v * 3));
            }
        }
        let before = b.heap_size();
        let copy = b.clone();
        b.compact();
        assert_eq!(b, copy);
        assert!(b.heap_size() < before, "{} !< {}", b.heap_size(), before);
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
        assert_eq!(RoaringType::insert_many(&mut b, &[3, 1, 2]), 3);
        assert_eq!(RoaringType::min_val(&b), Some(1));
        assert_eq!(RoaringType::max_val(&b), Some(3));
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
        let mut d = bm(&[1]);
        let _ = RoaringType::optimize(&mut d);
        assert!(RoaringType::contains(&d, 1));
    }

    #[test]
    fn legacy_grammars_match_1_1_1() {
        let mut blob = Vec::new();
        bm(&[1, 2, 70_000]).serialize_into(&mut blob).unwrap();
        blob.extend_from_slice(b"\0JUNK");
        assert_eq!(
            <RoaringBitmap as RoaringType>::deserialize_legacy(&blob).unwrap(),
            bm(&[1, 2, 70_000])
        );
        // 1.1.1 parsed values as u64 ('+' and leading zeros allowed), then
        // narrowed them to the width.
        let legacy = <RoaringBitmap as RoaringType>::parse_value_legacy;
        assert_eq!(legacy(b"+5"), Some(5));
        assert_eq!(legacy(b"005"), Some(5));
        assert_eq!(legacy(b"4294967295"), Some(u32::MAX));
        assert_eq!(legacy(b"4294967296"), None);
        assert_eq!(legacy(b"-1"), None);
        assert_eq!(legacy(b" 5"), None);
        // Strictly valid input reads the same either way.
        for v in [&b"0"[..], b"7", b"4294967295"] {
            assert_eq!(
                legacy(v),
                <RoaringBitmap as RoaringType>::parse_value_arg(v)
            );
        }
    }

    #[test]
    fn decode_exact_rejects_trailing_bytes() {
        use crate::bitmap_type::decode_exact;
        let mut blob = Vec::new();
        bm(&[1, 2, 3, 70_000]).serialize_into(&mut blob).unwrap();
        assert_eq!(
            decode_exact::<RoaringBitmap>(&blob).unwrap(),
            bm(&[1, 2, 3, 70_000])
        );
        for extra in [&b"\0"[..], b"\x01\x02", b"junk"] {
            let mut padded = blob.clone();
            padded.extend_from_slice(extra);
            assert!(decode_exact::<RoaringBitmap>(&padded).is_err());
        }
        for cut in 0..blob.len() {
            assert!(
                decode_exact::<RoaringBitmap>(&blob[..cut]).is_err(),
                "cut {cut}"
            );
        }
    }

    /// R.STAT's text, byte for byte as redis-roaring prints it, with the
    /// counters checked against the published module (CRoaring units).
    #[test]
    fn stat_matches_upstream_layout_and_units() {
        let b = bm(&[1, 2, 3]);
        assert_eq!(
            b.stat_fields().text(),
            "type: bitmap\ncardinality: 3\nnumber of containers: 1\nmax value: 3\n\
             min value: 1\nnumber of array containers: 1\n\tarray container values: 3\n\
             \tarray container bytes: 6\nbitset  containers: 0\n\
             \tbitset  container values: 0\n\tbitset  container bytes: 0\n\
             run containers: 0\n\trun container values: 0\n\trun container bytes: 0\n"
        );
        assert_eq!(
            b.stat_fields().json(),
            "{\"type\":\"bitmap\",\"cardinality\":\"3\",\"number_of_containers\":\"1\",\
             \"max_value\":\"3\",\"min_value\":\"1\",\"array_container\":{\
             \"number_of_containers\":\"1\",\"container_cardinality\":\"3\",\
             \"container_allocated_bytes\":\"6\"},\"bitset_container\":{\
             \"number_of_containers\":\"0\",\"container_cardinality\":\"0\",\
             \"container_allocated_bytes\":\"0\"},\"run_container\":{\
             \"number_of_containers\":\"0\",\"container_cardinality\":\"0\",\
             \"container_allocated_bytes\":\"0\"}}"
        );

        // Empty: max 0, min the width's maximum.
        let e = RoaringBitmap::new().stat_fields();
        assert_eq!(
            (e.cardinality, e.containers, e.max, e.min),
            (0, 0, 0, u64::from(u32::MAX))
        );

        // A bitset container: 8192 bytes, whatever its cardinality.
        let s = RoaringBitmap::from_sorted_iter(0..5000)
            .unwrap()
            .stat_fields();
        assert_eq!(
            (s.bitsets, s.bitset_values, s.bitset_bytes),
            (1, 5000, 8192)
        );

        // Run containers: 2 bytes plus 4 per run each.
        let mut r = bm(&[10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 100_000, 100_001]);
        r.optimize();
        let r = r.stat_fields();
        assert_eq!((r.runs, r.run_values, r.run_bytes), (1, 11, 6));
        assert_eq!((r.arrays, r.array_values, r.array_bytes), (1, 2, 4));

        // The full space: the 32-bit run-value counter wraps to 0 upstream.
        let f = RoaringBitmap::full().stat_fields();
        assert_eq!(f.cardinality, 1 << 32);
        assert_eq!(
            (f.containers, f.runs, f.run_values, f.run_bytes),
            (65536, 65536, 0, 393216)
        );
        assert_eq!((f.max, f.min), (u64::from(u32::MAX), 0));
    }
}
