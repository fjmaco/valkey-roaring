//! valkey-roaring: Canonical R.EXPORT blobs.
//!
//! EXPORT optimizes before serializing so that one logical set always yields
//! one byte sequence, whatever writes built it. roaring-rs's optimize()
//! leaves one case open: a container whose run encoding is exactly as large
//! as its array encoding (cardinality = 2 * runs + 1, e.g. {5, 6, 7}) keeps
//! whichever encoding it already has. SETRANGE builds run containers and
//! SETINTARRAY builds array containers, so such a set exported differently
//! depending on its history.
//!
//! Container encodings are private to roaring-rs, but the portable format
//! records them, so EXPORT reads them back from its own output and demotes
//! tied run containers to arrays, the encoding optimize() settles on when it
//! starts from an array. Blobs without run containers (the common sparse
//! case) are recognized from their first four bytes. A 64-bit blob is a
//! sequence of 32-bit ones, each checked as it is written (bitmap64.rs).

/// Portable-format cookies (RoaringFormatSpec).
const SERIAL_COOKIE_NO_RUNCONTAINER: u32 = 12346;
const SERIAL_COOKIE: u32 = 12347;
/// Containers with more values than this are bitsets unless run-encoded.
const ARRAY_LIMIT: usize = 4096;
const BITSET_BYTES: usize = 8192;

fn u16_at(b: &[u8], pos: usize) -> Option<usize> {
    Some(u16::from_le_bytes(b.get(pos..pos + 2)?.try_into().ok()?) as usize)
}

fn u32_at(b: &[u8], pos: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(pos..pos + 4)?.try_into().ok()?))
}

/// True when a 32-bit blob has no run container at all, from its cookie
/// alone: nothing can tie, so no scan is needed.
pub(crate) fn has_no_runs(blob: &[u8]) -> bool {
    u32_at(blob, 0) == Some(SERIAL_COOKIE_NO_RUNCONTAINER)
}

/// A run container whose run and array encodings tie: its position in the
/// container sequence, its key and its cardinality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Tie {
    pub index: usize,
    pub key: u16,
    pub card: usize,
}

/// Scans one complete 32-bit portable blob for tied run containers, in
/// container order. None if it does not parse to exactly its length, which
/// for a blob just serialized means a bug; callers then keep the blob as it
/// is.
pub(crate) fn tied_run_containers(blob: &[u8]) -> Option<Vec<Tie>> {
    let cookie = u32_at(blob, 0)?;
    let (size, run_flags, mut pos) = if cookie == SERIAL_COOKIE_NO_RUNCONTAINER {
        let size = u32_at(blob, 4)? as usize;
        (size, None, 8)
    } else if cookie & 0xFFFF == SERIAL_COOKIE {
        let size = (cookie >> 16) as usize + 1;
        let flags = blob.get(4..4 + size.div_ceil(8))?;
        (size, Some(flags), 4 + size.div_ceil(8))
    } else {
        return None;
    };
    let descriptions = pos;
    pos += 4 * size;
    let is_run = |i: usize| run_flags.is_some_and(|f| f[i / 8] & (1 << (i % 8)) != 0);
    let card_of = |i: usize| Some(u16_at(blob, descriptions + 4 * i + 2)? + 1);

    // Offset table: always in the no-run layout; with runs, only from four
    // containers up. Container data follows it, in container order.
    if run_flags.is_none() || size >= 4 {
        // The table locates every container, so only plausible ties are
        // visited: run containers with an odd cardinality (a tie has
        // 2 * runs + 1 values) of at most ARRAY_LIMIT. The header is read
        // in order and no other container's data is touched, which keeps a
        // steady-state EXPORT close to a plain serialization.
        let offsets = pos;
        let offset_of = |i: usize| Some(u32_at(blob, offsets + 4 * i)? as usize);
        let mut tied = Vec::new();
        for i in 0..size {
            if !is_run(i) {
                continue;
            }
            let card = card_of(i)?;
            if card > ARRAY_LIMIT || card % 2 == 0 {
                continue;
            }
            let runs = u16_at(blob, offset_of(i)?)?;
            if 2 + 4 * runs == 2 * card {
                tied.push(Tie {
                    index: i,
                    key: u16_at(blob, descriptions + 4 * i)? as u16,
                    card,
                });
            }
        }
        // The blob must end exactly where its last container does.
        let end = match size.checked_sub(1) {
            None => offsets,
            Some(last) => {
                let start = offset_of(last)?;
                let card = card_of(last)?;
                start
                    + if is_run(last) {
                        2 + 4 * u16_at(blob, start)?
                    } else if card <= ARRAY_LIMIT {
                        2 * card
                    } else {
                        BITSET_BYTES
                    }
            }
        };
        return (end == blob.len()).then_some(tied);
    }

    // Fewer than four containers, with runs: no offset table, so walk them.
    let mut tied = Vec::new();
    for i in 0..size {
        let key = u16_at(blob, descriptions + 4 * i)?;
        let card = card_of(i)?;
        let bytes = if is_run(i) {
            let runs = u16_at(blob, pos)?;
            let bytes = 2 + 4 * runs;
            if card <= ARRAY_LIMIT && bytes == 2 * card {
                tied.push(Tie {
                    index: i,
                    key: key as u16,
                    card,
                });
            }
            bytes
        } else if card <= ARRAY_LIMIT {
            2 * card
        } else {
            BITSET_BYTES
        };
        pos += bytes;
    }
    (pos == blob.len()).then_some(tied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use roaring::RoaringBitmap;

    fn blob32(b: &RoaringBitmap) -> Vec<u8> {
        let mut out = Vec::new();
        b.serialize_into(&mut out).unwrap();
        out
    }

    fn tied_keys(blob: &[u8]) -> Option<Vec<u16>> {
        tied_run_containers(blob).map(|t| t.iter().map(|t| t.key).collect())
    }

    #[test]
    fn finds_tied_run_containers_only() {
        // {5,6,7} as a run: 2 + 4*1 == 2*3 bytes, a tie.
        let mut tie = RoaringBitmap::new();
        tie.insert_range(5..8);
        let b = blob32(&tie);
        assert_eq!(
            tied_run_containers(&b),
            Some(vec![Tie {
                index: 0,
                key: 0,
                card: 3
            }])
        );
        assert!(!has_no_runs(&b));

        // A strictly smaller run is not a tie; neither is any array.
        let mut long_run = RoaringBitmap::new();
        long_run.insert_range(0..100);
        let b = blob32(&long_run);
        assert_eq!(tied_keys(&b), Some(vec![]));
        let arrays: RoaringBitmap = [5u32, 6, 7, 70_000].into_iter().collect();
        let b = blob32(&arrays);
        assert_eq!(tied_keys(&b), Some(vec![]));
        assert!(has_no_runs(&b));
        let b = blob32(&RoaringBitmap::new());
        assert_eq!(tied_keys(&b), Some(vec![]));
    }

    #[test]
    fn walks_every_layout() {
        // Mixed array, bitset and run containers, with ties in some of them;
        // below and above the four-container offset-table threshold.
        for extra in [0u32, 1, 2, 6] {
            let mut b = RoaringBitmap::new();
            b.insert_range(5..8); // key 0: tie
            for k in 1..=extra {
                match k % 3 {
                    0 => b.extend((0..5_000u32).map(|v| (k << 16) | (v * 3))), // bitset
                    1 => b.extend([k << 16 | 9, k << 16 | 400]),               // array
                    _ => {
                        b.insert_range(k << 16 | 10..k << 16 | 13); // tie
                        b.insert_range(k << 16 | 20..k << 16 | 23);
                        b.insert(k << 16 | 30); // 7 values, 3 runs: tie
                    }
                }
            }
            let blob = blob32(&b);
            let tied = tied_keys(&blob).unwrap();
            let expected: Vec<u16> = std::iter::once(0)
                .chain((1..=extra).filter(|k| k % 3 == 2).map(|k| k as u16))
                .collect();
            assert_eq!(tied, expected, "extra {extra}");
        }
    }

    #[test]
    fn rejects_garbage_and_truncation() {
        assert_eq!(tied_run_containers(&[]), None);
        assert_eq!(tied_run_containers(&[1, 2, 3, 4, 5, 6, 7, 8]), None);
        let mut tie = RoaringBitmap::new();
        tie.insert_range(5..8);
        let b = blob32(&tie);
        for cut in 0..b.len() {
            assert_eq!(tied_run_containers(&b[..cut]), None, "cut at {cut}");
        }
        let mut longer = b.clone();
        longer.push(0);
        assert_eq!(tied_run_containers(&longer), None);
        // The same with an offset table (four or more containers, runs).
        let mut wide = RoaringBitmap::new();
        for k in 0..6u32 {
            wide.insert_range(k << 16 | 5..k << 16 | 8);
        }
        wide.insert(9 << 16 | 1);
        let b = blob32(&wide);
        assert_eq!(tied_keys(&b), Some((0..6).collect()));
        for cut in 0..b.len() {
            assert_eq!(tied_run_containers(&b[..cut]), None, "cut at {cut}");
        }
        let mut longer = b.clone();
        longer.extend_from_slice(&[0, 0]);
        assert_eq!(tied_run_containers(&longer), None);
    }
}
