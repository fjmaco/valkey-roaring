# valkey-roaring

[![CI](https://github.com/fjmaco/valkey-roaring/actions/workflows/ci.yml/badge.svg)](https://github.com/fjmaco/valkey-roaring/actions/workflows/ci.yml)
[![Fuzz](https://github.com/fjmaco/valkey-roaring/actions/workflows/fuzz.yml/badge.svg)](https://github.com/fjmaco/valkey-roaring/actions/workflows/fuzz.yml)
[![codecov](https://codecov.io/gh/fjmaco/valkey-roaring/graph/badge.svg)](https://codecov.io/gh/fjmaco/valkey-roaring)
[![Docker Pulls](https://img.shields.io/docker/pulls/fjmaco/valkey-roaring?label=docker%20pulls)](https://hub.docker.com/r/fjmaco/valkey-roaring)

Roaring Bitmaps for [Valkey](https://valkey.io/). **[Documentation →](https://fjmaco.github.io/valkey-roaring/)**

[Roaring Bitmaps](https://roaringbitmap.org/) are compressed bitmap data structures that outperform plain bitmaps on both memory and speed for sparse or clustered integer sets. This module adds them to Valkey as native types, exposed through **51 commands** across 32-bit (`R.*`) and 64-bit (`R64.*`) variants — including binary export/import in the [CRoaring portable format](https://github.com/RoaringBitmap/CRoaring), so bitmaps can move between Valkey and any service that speaks the format (Java, Go, Python, C++, Rust) without intermediate integer arrays.

Built in Rust on the official [valkey-module](https://crates.io/crates/valkey-module) SDK and the [roaring](https://crates.io/crates/roaring) crate. The module is compatible with both **Valkey** and **Redis**: it initializes through the RedisModule API, which both servers expose, and loads cleanly on Valkey 8.1+ as well as Redis 7.4 and 8.

## Features

- **Valkey and Redis compatible** — one `.so` loads on Valkey 8.1+ and Redis 7.4/8, with full functionality and RDB persistence on both
- **Two value ranges** — 32-bit (`R.*`, values 0 to 2³²−1) and 64-bit (`R64.*`, values 0 to 2⁶⁴−1) bitmap types with identical command semantics
- **Binary export/import** — `R.EXPORT` / `R.IMPORT` serialize to the CRoaring portable format for efficient cross-service transfer
- **8 bitwise operations** — AND, OR, XOR, NOT, ANDOR, DIFF, DIFF1, ONE, cluster-aware key reporting included
- **RDB persistence** — bitmaps survive `BGSAVE` and server restarts
- **Container statistics** — `R.STAT` reports cardinality, min/max, and the full array/bitset/run container breakdown for both widths

## Implementation

valkey-roaring is a ground-up Rust implementation targeting Valkey. The bitmaps come from [roaring-rs](https://github.com/RoaringBitmap/roaring-rs), the RoaringBitmap project's pure-Rust implementation, and the module layer is built on [valkeymodule-rs](https://github.com/valkey-io/valkeymodule-rs), the Valkey project's official Rust SDK. Staying on an all-Rust stack means the module inherits the roaring crate's correctness and performance work, builds with a single `cargo build` and no C toolchain, and picks up improvements to both the bitmap library and the Valkey module ecosystem with a dependency bump.

Command replies are a stable contract, argument grammar, error wording and reply types included. The [command reference](https://fjmaco.github.io/valkey-roaring/commands/) documents each command's edge cases and the limits that guard against oversized writes and replies.

## Requirements

| Requirement | Version |
|-------------|---------|
| Valkey      | 8.1+    |
| Redis (alternative to Valkey) | 7.4+ |
| Rust (build)| 1.90+   |
| Docker      | 20.10+ (optional) |

Dependencies: [roaring](https://crates.io/crates/roaring) 0.11.5 and [valkey-module](https://crates.io/crates/valkey-module) 0.1, both from crates.io, unmodified. No C dependencies.

## Getting Started

### Docker Hub

```bash
docker run -d -p 6379:6379 fjmaco/valkey-roaring
```

Images are published automatically: `latest` from `main`, version tags from
`v*` releases.

### Docker Compose (build from source)

```bash
docker compose up -d
```

This builds the module from source and starts Valkey 8.1 on port `6379` with `valkey-roaring` loaded.

```bash
docker compose exec valkey valkey-cli
```

### Build from Source

```bash
cargo build --release
# Output: target/release/libvalkey_roaring.so
```

Load into a running Valkey server:

```bash
valkey-server --loadmodule ./target/release/libvalkey_roaring.so
```

Or add to `valkey.conf`:

```
loadmodule /path/to/libvalkey_roaring.so
```

### Verify

```bash
valkey-cli R.SETBIT test 42 1    # (integer) 0
valkey-cli R.GETBIT test 42      # (integer) 1
valkey-cli R.BITCOUNT test       # (integer) 1
```

## API

All commands exist in 32-bit (`R.*`) and 64-bit (`R64.*`) forms. The `R.*` variant accepts `u32` values (0 to 4,294,967,295); `R64.*` accepts `u64` values (0 to 18,446,744,073,709,551,615). Behavior is identical. Values above 2⁶³−1 are replied as decimal strings (RESP integers are signed 64-bit).

### Bit Manipulation

- `R.SETBIT key offset 0|1` — Set or clear a bit (same as [SETBIT](https://valkey.io/commands/setbit))
- `R.GETBIT key offset` — Get bit value (same as [GETBIT](https://valkey.io/commands/getbit))
- `R.GETBITS key offset [offset ...]` — Get multiple bit values at once
- `R.CLEARBITS key offset [offset ...] [COUNT]` — Clear multiple bits; replies OK, or the count actually cleared with the `COUNT` flag (null for a missing key)
- `R.CLEAR key` — Reset bitmap to empty, returns previous cardinality

### Bulk Set/Get

- `R.SETINTARRAY key val [val ...]` — Replace bitmap with integer set
- `R.GETINTARRAY key` — Get all set bits as sorted integer array
- `R.APPENDINTARRAY key val [val ...]` — Add integers to bitmap
- `R.DELETEINTARRAY key val [val ...]` — Remove integers from bitmap
- `R.RANGEINTARRAY key start end` — Paginate the sorted value array: elements at 0-based positions [start, end], truncated at the cardinality (max window 100,000,000)

### Bit Array

- `R.SETBITARRAY key "010110..."` — Create bitmap from ASCII bit string
- `R.GETBITARRAY key` — Get bitmap as ASCII bit string

### Range and Fill

- `R.SETRANGE key start end` — Set all bits in the end-exclusive range [start, end)
- `R.SETFULL key` — Set all possible bits (errors if key exists; `R64.SETFULL` is refused, see [Known Limitations](#known-limitations))

### Aggregation

- `R.BITCOUNT key` — Cardinality / number of set bits (same as [BITCOUNT](https://valkey.io/commands/bitcount) without start/end)
- `R.BITPOS key 0|1` — Position of first set (1) or unset (0) bit (same as [BITPOS](https://valkey.io/commands/bitpos) without start/end)
- `R.MIN key` — Smallest set bit, returns -1 if empty
- `R.MAX key` — Largest set bit, returns -1 if empty

### Set Operations

- `R.CONTAINS key1 key2 [mode]` — Check relationship between bitmaps
- `R.JACCARD key1 key2` — Jaccard similarity index
- `R.DIFF dest key1 key2` — Store `key1 - key2` in dest

**CONTAINS modes:** default (no mode argument) checks for any overlap; explicit modes are `ALL` (subset), `ALL_STRICT` (proper subset), `EQ` (equal).

### Bitwise Operations

```
R.BITOP NOT  destkey srckey [last]
R.BITOP <op> destkey srckey srckey [srckey ...]
```

Same as [BITOP](https://valkey.io/commands/bitop) with extended operations:

| Operation | Semantics |
|-----------|-----------|
| `AND`     | Intersection of all sources |
| `OR`      | Union of all sources |
| `XOR`     | Symmetric difference |
| `NOT`     | Complement of single source over `[0, max(last, src max)]` |
| `ANDOR`   | `(src[1] \| src[2] \| ...) & src[0]` |
| `DIFF`    | `src[0] - src[1] - src[2] - ...` |
| `DIFF1`   | `(src[1] \| src[2] \| ...) - src[0]` |
| `ONE`     | Bits present in exactly one source |

All BITOP operations return the cardinality of the result.

`NOT` accepts an optional `last` argument bounding the universe to complement within; a `last` below the source's max is raised to it. A missing or empty source stores an empty bitmap (returns 0), or the full `[0, last]` range when `last` is given. `R.BITOP` reports its key positions dynamically through the module getkeys API, so `COMMAND GETKEYS`, ACL checks, and cluster routing handle the trailing non-key `last` argument correctly.

### Export / Import

- `R.EXPORT key` — Serialize to CRoaring portable binary format
- `R.IMPORT key binary` — Deserialize and OR-merge into key, returns cardinality after import

The binary output of `R.EXPORT` is compatible with any [CRoaring-compatible library](#croaring-compatible-libraries) (Java, Go, Python, C++, Rust). This is the recommended way to transfer bitmaps between services.

`R.EXPORT` is canonical: one set always exports the same bytes, whatever sequence of writes built it, so consumers can hash or dedupe blobs. It counts as a read — it never invalidates `WATCH` or client-side caching and is not replicated. `R.IMPORT` rejects malformed blobs with `ERR bad binary data for roaring`, including the oversized array containers some producers emit (Go roaring v1.9.4's `FastOr`/`ParOr`).

From a shell, use `valkey-cli`'s raw output and `-x` (both are binary-safe;
pasting binary as a command argument is not):

```bash
valkey-cli R.EXPORT source > bitmap.bin        # raw reply redirected to a file
valkey-cli -x R.IMPORT destination < bitmap.bin  # -x passes stdin as the last arg
```

From Lua:

```lua
local data = redis.call('R.EXPORT', 'source')
redis.call('R.IMPORT', 'destination', data)
```

### Maintenance

- `R.OPTIMIZE key` — Optimize internal container storage for better compression (the key must exist)
- `R.STAT key [TEXT|JSON]` — Container statistics (works for both `R.*` and `R64.*` keys)

### 64-bit Commands

All commands above have 64-bit equivalents with the `R64.` prefix:

`R64.SETBIT`, `R64.GETBIT`, `R64.GETBITS`, `R64.CLEARBITS`, `R64.CLEAR`, `R64.SETINTARRAY`, `R64.GETINTARRAY`, `R64.APPENDINTARRAY`, `R64.DELETEINTARRAY`, `R64.RANGEINTARRAY`, `R64.SETBITARRAY`, `R64.GETBITARRAY`, `R64.SETRANGE`, `R64.SETFULL`, `R64.BITCOUNT`, `R64.BITPOS`, `R64.MIN`, `R64.MAX`, `R64.OPTIMIZE`, `R64.CONTAINS`, `R64.JACCARD`, `R64.DIFF`, `R64.BITOP`, `R64.EXPORT`, `R64.IMPORT`

`R.STAT` is shared — it auto-detects whether the key is 32-bit or 64-bit.

**Total: 51 commands** (25 `R.*` + 25 `R64.*` + 1 `R.STAT`)

## API Example

```
$ valkey-cli

# set individual bits
127.0.0.1:6379> R.SETBIT users:active 42 1
(integer) 0
127.0.0.1:6379> R.SETBIT users:active 123 1
(integer) 0

# check a bit
127.0.0.1:6379> R.GETBIT users:active 42
(integer) 1

# count set bits
127.0.0.1:6379> R.BITCOUNT users:active
(integer) 2

# create a bitmap from a range — end-exclusive: sets 1 through 100
127.0.0.1:6379> R.SETRANGE range_test 1 101
OK

# get all numbers as an integer array
127.0.0.1:6379> R.GETINTARRAY range_test
  1) (integer) 1
  2) (integer) 2
  ...
100) (integer) 100

# paginate: elements at positions 49..59 of the sorted array
127.0.0.1:6379> R.RANGEINTARRAY range_test 49 59
 1) (integer) 50
 2) (integer) 51
...
11) (integer) 60

# append numbers to an existing bitmap
127.0.0.1:6379> R.APPENDINTARRAY range_test 200 300 400
OK

# bitwise operations
127.0.0.1:6379> R.SETINTARRAY a 1 2 3 4 5
OK
127.0.0.1:6379> R.SETINTARRAY b 3 4 5 6 7
OK
127.0.0.1:6379> R.BITOP AND result a b
(integer) 3
127.0.0.1:6379> R.GETINTARRAY result
1) (integer) 3
2) (integer) 4
3) (integer) 5

# export bitmap as portable binary (for cross-service transfer)
# use from a client library, not valkey-cli (binary contains null bytes)

# get statistics
127.0.0.1:6379> R.STAT users:active
"type: bitmap\ncardinality: 2\nnumber of containers: 1\nmax value: 123\nmin value: 42\n..."

# Jaccard similarity
127.0.0.1:6379> R.JACCARD a b
"0.42857142857142855"

# check if a is a subset of b
127.0.0.1:6379> R.CONTAINS a b ALL
(integer) 0
```

## Architecture

```
src/
  lib.rs              Module entry, type registration, 51 command wrappers
  bitmap_type.rs      RoaringType trait (abstracts u32 vs u64)
  bitmap32.rs         impl RoaringType for RoaringBitmap (u32)
  bitmap64.rs         impl RoaringType for RoaringTreemap (u64)
  commands.rs         Generic command handlers
  commands_bitop.rs   BITOP dispatch + 8 sub-operations
  error.rs            Error constants
  parse.rs            Argument parsing
```

Every command handler is a single generic function parameterized by the `RoaringType` trait. At compile time it is instantiated twice via monomorphization, so one implementation serves both bitmap widths and the two command families cannot drift apart:

```rust
fn handle_setbit<T: RoaringType>(ctx, args, vtype) -> ValkeyResult { ... }

// Registered as:
["R.SETBIT",   r_setbit,   ...]   // T = RoaringBitmap (u32)
["R64.SETBIT", r64_setbit, ...]   // T = RoaringTreemap (u64)
```

### Persistence and Replication

- **RDB:** Bitmaps serialize via the CRoaring portable binary format. Data survives `BGSAVE` and server restarts.
- **Replication:** Every write that changes a key propagates verbatim to replicas and to the AOF command stream. Writes that change nothing — `R.SETBIT` to the value a bit already has, `R.APPENDINTARRAY` of present values, `R.DELETEINTARRAY` / `R.CLEARBITS` of absent ones, `R.SETRANGE` over an already-set range, `R.IMPORT` of a subset, `R.CLEAR` of an empty key — reply as usual but are not propagated, do not count toward RDB save points, and do not invalidate `WATCH` or client-side caching. A write to a missing key always counts: it creates the key (`R.SETBIT key n 0` creates an empty bitmap).
- **AOF:** Supported in both modes. With the default `aof-use-rdb-preamble yes`, rewrites use the RDB serialization as the base. With `aof-use-rdb-preamble no`, the per-type rewrite callback re-emits each key as one `R.IMPORT` / `R64.IMPORT` of its portable blob.
- **Registered type names:** `vrroaring` (32-bit), `vroarng64` (64-bit).
- **Upgrading from 1.1.1:** RDB files and DUMP payloads load unchanged, and commands replayed from a 1.1.1 AOF or received from a 1.1.1 primary are read with 1.1.1's input rules, so they keep its results even where clients now get an error. Run `BGREWRITEAOF` on 1.1.1 before upgrading all the same: it leaves no 1.1.1 command history to replay (see [the guide](https://fjmaco.github.io/valkey-roaring/guide/persistence-and-replication)).

### Memory Management

The module sets Rust's global allocator to `ValkeyAlloc`, routing all allocations (bitmaps, buffers, temporary structures) through Valkey's memory tracking. This ensures `INFO MEMORY` accurately reflects module usage.

Each value reports one unit of free effort per 16 containers, so with lazy freeing enabled (`UNLINK`, or the `lazyfree-lazy-*` settings, `lazyfree-lazy-server-del` being on by default) bitmaps of more than 1,024 containers are freed on a background thread instead of blocking the main thread. Smaller ones are freed in place, which is cheaper than handing them over: overwriting a BITOP or DIFF destination of ~150 containers costs ~10 µs more through the lazy-free thread.

### Fault Isolation

Every command runs inside a panic guard. If a bug in the module or in roaring-rs panics, the client gets `ERR internal error (panic): <message>`, the server log gets a warning, and the server keeps running. Without the guard, a panic unwinding into Valkey's C code aborts the process.

## Tests

Three layers, all run by CI ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) on every push and pull request, alongside `rustfmt`/`clippy` gates and a unit-layer coverage report. The [benchmark workflow](.github/workflows/benchmark.yml) refreshes the table below whenever performance-relevant code changes. See [CONTRIBUTING.md](CONTRIBUTING.md) for running the gates locally.

**Unit and property tests** (no server needed) — 95 tests covering every
hand-written algorithm:

```bash
cargo test
```

- `nth_absent`, `flip_inclusive`, bit-array codecs, bulk-op change counts,
  select bounds, u64 reply saturation — including type-boundary edge cases
- Range containment (the `SETRANGE` no-op check) against brute force, across
  R64's 32-bit sub-bitmap boundaries; no-op writes leave the serialized
  layout byte-identical
- The panic guard turns panics into single-line error messages; free effort
  is never 0 (which the server reads as "always free asynchronously")
- All 7 BITOP kernels checked against a naive reference over ~1,100 randomized
  source combinations (both bitmap widths)
- 32-bit / 64-bit parity over 12,000 randomized operations
- Serialization round-trips, plus 800+ corrupted/truncated/garbage inputs fed
  through the `R.IMPORT` deserialization path asserting it never panics, and
  oversized array containers (Go roaring `FastOr` output) rejected
- Canonical `R.EXPORT`: sets built five different ways (bulk, range writes,
  bit by bit, trimmed supersets) export byte-identical blobs on both widths
- Fast paths against their reference forms: bulk construction vs inserts,
  `RANGEINTARRAY` paging vs per-position select, both intersection strategies,
  allocation-free argument parsing vs the former `String` parse; the
  `MEMORY USAGE` heap model is pinned to roaring-rs's statistics units
- Reply formats and argument grammars: the 32-bit and 64-bit parsers checked
  against independent oracle parsers, JACCARD's decimal/`%.17g` formatting
  against C `printf` values, `R.STAT`'s text and JSON byte for byte; IMPORT
  validation (trailing bytes, repeated or decreasing 64-bit high words,
  every truncation) on both widths, and 1.1.1's grammars and decoding for
  replayed commands
- Cost guards: canonical EXPORT and interleaved unions stay linear, and an
  R64 SETRANGE check on a million-container key does not walk the key

The coverage badge reports the unit/property layer only; the command
handlers it cannot instrument are exercised by the integration suite below
(see `codecov.yml` for the scoping rationale).

**Integration suite** — 432 assertions against a live Valkey instance:

```bash
# From the repository root (requires running docker compose)
bash tests/integration.sh
```

- Every command for both 32-bit and 64-bit types
- All 8 BITOP sub-operations with correctness checks
- CONTAINS with all 4 modes (NONE, ALL, ALL_STRICT, EQ)
- EXPORT/IMPORT binary round-trip via Lua
- RDB persistence across server restart
- Replication: module writes verified on a live replica
- No-op writes leave the dirty counter (and so replication and the AOF) untouched
- AOF: replay after restart, and rewrites both via the RDB preamble and without it
- Dynamic GETKEYS, `BITOP NOT ... last`, duplicate-offset and BITPOS edge cases
- The reply contract on one server: argument grammars, check order, exact error
  texts, case-sensitive tokens, JACCARD and STAT reply types (checked through
  Lua under RESP2 and RESP3), full-width and 64-bit pagination
- Limits: oversized GETINTARRAY, `R64.SETFULL`, wide `R64.SETRANGE` and
  `R64.BITOP NOT` refused up front, the full 32-bit space still accepted
- IMPORT validation: trailing bytes and non-increasing 64-bit high words
- Upgrade safety: an AOF holding commands only 1.1.1 accepted (lenient IMPORT
  blobs, `+5`/`007`/`01`, lowercase BITOP) replays to 1.1.1's sets, while
  clients still get the strict grammar
- Systematic error coverage: wrong-arity for all 51 commands, WRONGTYPE for
  every key command against a mistyped key, semantic errors (missing keys,
  bad binary, out-of-range values)

**Fuzzing** — four [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz)
targets run 60s each on every push/PR and 10 minutes nightly, with a
persistent corpus cached between runs:

```bash
cargo +nightly fuzz run import_bytes      # untrusted bytes into the R.IMPORT path
cargo +nightly fuzz run parity_ops        # 32-bit vs 64-bit behavioral parity
cargo +nightly fuzz run bitop_kernels     # BITOP kernels vs a naive reference
cargo +nightly fuzz run export_canonical  # imported encodings still export one canonical blob
```

**Performance benchmark** — see [Performance](#performance); CI runs a smoke
subset on every push.

**External validation** — nineteen end-to-end suites (two of them, load/soak
and very large keys, run only on request) live in a separate repository,
[fjmaco/-valkey-roaring-testing](https://github.com/fjmaco/-valkey-roaring-testing):
real-dataset semantics against a reference model, CRoaring interop,
replication, cluster, torture, workflow contracts, canonical EXPORT, write
signals, streamed replies and memory accounting. They are kept out of this tree deliberately — they
validate the module the way an outside consumer would, through the wire
protocol, the Docker image and the published binary format only, and share
no code with it. That is also where new end-to-end tests belong.

```bash
git clone https://github.com/fjmaco/-valkey-roaring-testing.git valkey-roaring-testing
cd valkey-roaring-testing && bash run_all.sh
```

The runner builds the module from a sibling `../valkey-roaring` checkout
when one exists, so a working tree can be validated before it is pushed;
otherwise it clones this repository. `VR_SOURCE=/path/to/checkout` and
`VR_REF=<branch|tag>` select the source explicitly.

## Performance

Benchmark methodology: CRoaring's `census1881` dataset, full client
round-trip latency per command against the dockerized Valkey, compared with
the equivalent native commands. The harness lives in `tests/performance/`.

```bash
bash tests/performance.sh                    # full run, updates this table
PERF_MAX_FILES=5 bash tests/performance.sh   # quick smoke run
```

<!-- #region performance-table -->
<!-- BEGIN_PERFORMANCE -->
|               OP |     TIME/OP (us) |     ST.DEV. (us) |
| ---------------- | ---------------- | ---------------- |
|         R.SETBIT |           125.14 |            16.12 |
|       R64.SETBIT |           124.62 |            15.30 |
|           SETBIT |           123.71 |            14.27 |
|         R.GETBIT |           124.04 |            13.60 |
|       R64.GETBIT |           124.03 |            13.98 |
|           GETBIT |           122.85 |            12.98 |
|       R.BITCOUNT |           119.91 |            15.76 |
|     R64.BITCOUNT |           115.38 |            11.31 |
|         BITCOUNT |           132.52 |            11.49 |
|         R.BITPOS |           115.94 |             5.77 |
|       R64.BITPOS |           116.69 |            14.65 |
|           BITPOS |           122.46 |            10.50 |
|      R.BITOP NOT |           276.14 |           568.05 |
|    R64.BITOP NOT |           277.00 |           533.02 |
|        BITOP NOT |           298.35 |            92.27 |
|      R.BITOP AND |           129.78 |            27.38 |
|    R64.BITOP AND |           152.80 |            16.89 |
|        BITOP AND |           299.21 |           201.50 |
|       R.BITOP OR |           156.51 |            38.52 |
|     R64.BITOP OR |           155.64 |            38.46 |
|         BITOP OR |           401.46 |           321.16 |
|      R.BITOP XOR |           151.89 |            40.89 |
|    R64.BITOP XOR |           131.65 |            42.77 |
|        BITOP XOR |           351.43 |           288.43 |
|    R.BITOP ANDOR |           124.72 |            25.41 |
|  R64.BITOP ANDOR |           124.06 |            16.60 |
|      BITOP ANDOR |           130.56 |            11.50 |
|      R.BITOP ONE |           156.45 |            39.87 |
|    R64.BITOP ONE |           152.59 |            46.87 |
|        BITOP ONE |           138.07 |             5.34 |
|            R.MIN |           114.18 |            11.54 |
|          R64.MIN |           137.03 |            11.75 |
|              MIN |           139.51 |             4.57 |
|            R.MAX |           130.37 |            12.03 |
|          R64.MAX |           137.09 |             7.48 |
|              MAX |           113.39 |            10.12 |
<!-- END_PERFORMANCE -->
<!-- #endregion performance-table -->

Notes: native `MIN`/`MAX` don't exist and `BITOP ANDOR`/`BITOP ONE` are not
supported by Valkey 8.1, so those native rows measure error-reply round-trips.
St.dev. is the per-command standard deviation.

## CRoaring-Compatible Libraries

The binary format produced by `R.EXPORT` / `R.IMPORT` is the standard CRoaring portable serialization. It can be read and written by:

| Language | Library |
|----------|---------|
| Java     | [RoaringBitmap](https://github.com/RoaringBitmap/RoaringBitmap) |
| Go       | [roaring](https://github.com/RoaringBitmap/roaring) |
| Python   | [pyroaring](https://github.com/Ezibenroc/PyRoaringBitMap) |
| C/C++    | [CRoaring](https://github.com/RoaringBitmap/CRoaring) |
| Rust     | [roaring-rs](https://github.com/RoaringBitmap/roaring-rs) |

## Known Limitations

- **Size limits.** One write builds at most 2³⁸ contiguous values (4,194,304 full containers, about 200 MB as runs, about 0.1 s; the bound is per call, so one write can take up to about 200 MB past `maxmemory` before `deny-oom` refuses further writes): `R64.SETFULL`, wider `R64.SETRANGE` calls and `R64.BITOP NOT` over a universe past 2³⁸ are refused with `Roaring: range too large: maximum 274877906944 elements` instead of allocating until the server is killed. The whole 32-bit space is within the limit (`R.SETFULL`, about 3 MB). Replies that list values (`GETINTARRAY`, `RANGEINTARRAY`, `GETBITARRAY`) are capped at 100,000,000 elements.
- **`R.EXPORT` / `R.IMPORT`** binaries cannot be pasted as command arguments; use `valkey-cli -x` / raw output redirection, Lua, or a client library (see [Export / Import](#export--import)).

## Acknowledgements

valkey-roaring was built as a rewrite of, and improvement on, the redis-roaring project.
