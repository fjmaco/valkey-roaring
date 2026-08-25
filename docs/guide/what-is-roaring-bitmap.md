# What Is a Roaring Bitmap

A bitmap (or bitset) represents a set of integers by dedicating one bit per
possible value: bit *N* is 1 when *N* is in the set. Bitmaps make membership
tests, unions, and intersections extremely fast — but a plain bitmap pays for
its full range up front. Storing the single value 4,000,000,000 in a classic
Valkey bitmap allocates half a gigabyte of zeroes.

[Roaring Bitmaps](https://roaringbitmap.org/) fix this by chopping the 32-bit
space into 65,536-value chunks and choosing the best physical representation
for each chunk independently:

- **Array containers** for sparse chunks — a sorted list of 16-bit values
- **Bitset containers** for dense chunks — a classic 8 KB bitmap
- **Run containers** for contiguous stretches — start/length pairs

The result is a structure that adapts to your data: sparse sets cost a few
bytes per element, dense ranges compress to almost nothing, and set
operations work directly on the compressed form. Roaring is the standard
compressed bitmap in Lucene, Spark, ClickHouse, Druid, and many other
systems.

## What valkey-roaring adds to Valkey

valkey-roaring registers two native Valkey data types:

| Type | Commands | Values |
|------|----------|--------|
| 32-bit bitmap | `R.*` | 0 … 2³²−1 |
| 64-bit bitmap | `R64.*` | 0 … 2⁶⁴−1 |

Both families expose the same 25 commands (plus the shared `R.STAT`), covering
single-bit access, bulk integer arrays, range fills, aggregation, set algebra,
and binary export/import. See the [command reference](/commands/) for the
full surface.

Typical uses:

- **User activity / presence** — one bit per user ID per day
- **Audience segmentation** — intersect and union segments with `R.BITOP`
- **Deduplication** — has this message ID been seen?
- **Analytics interchange** — [export the compressed set](/guide/export-import)
  to any service that speaks the Roaring portable format

## Origins

valkey-roaring is based on
[redis-roaring](https://github.com/aviggiano/redis-roaring) by Antonio
Viggiano and contributors, and tracks its changes and improvements —
applications built against redis-roaring's commands work here unchanged. It
is a ground-up Rust implementation: bitmaps come from
[roaring-rs](https://github.com/RoaringBitmap/roaring-rs), the RoaringBitmap
project's pure-Rust implementation, on
[valkeymodule-rs](https://github.com/valkey-io/valkeymodule-rs), the Valkey
project's official Rust SDK.
