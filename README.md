# valkey-roaring

[![CI](https://github.com/fjmaco/valkey-roaring/actions/workflows/ci.yml/badge.svg)](https://github.com/fjmaco/valkey-roaring/actions/workflows/ci.yml)
[![Fuzz](https://github.com/fjmaco/valkey-roaring/actions/workflows/fuzz.yml/badge.svg)](https://github.com/fjmaco/valkey-roaring/actions/workflows/fuzz.yml)
[![codecov](https://codecov.io/gh/fjmaco/valkey-roaring/graph/badge.svg)](https://codecov.io/gh/fjmaco/valkey-roaring)
[![Docker Pulls](https://img.shields.io/docker/pulls/fjmaco/valkey-roaring?label=docker%20pulls)](https://hub.docker.com/r/fjmaco/valkey-roaring)

Roaring Bitmaps for [Valkey](https://valkey.io/).

[Roaring Bitmaps](https://roaringbitmap.org/) are compressed bitmap data structures that outperform plain bitmaps on both memory and speed for sparse or clustered integer sets. This module adds them to Valkey as native types, exposed through **51 commands** across 32-bit (`R.*`) and 64-bit (`R64.*`) variants — including binary export/import in the [CRoaring portable format](https://github.com/RoaringBitmap/CRoaring), so bitmaps can move between Valkey and any service that speaks the format (Java, Go, Python, C++, Rust) without intermediate integer arrays.

Built in Rust on the official [valkey-module](https://crates.io/crates/valkey-module) SDK and the [roaring](https://crates.io/crates/roaring) crate.

## Features

- **Two value ranges** — 32-bit (`R.*`, values 0 to 2³²−1) and 64-bit (`R64.*`, values 0 to 2⁶⁴−1) bitmap types with identical command semantics
- **Binary export/import** — `R.EXPORT` / `R.IMPORT` serialize to the CRoaring portable format for efficient cross-service transfer
- **8 bitwise operations** — AND, OR, XOR, NOT, ANDOR, DIFF, DIFF1, ONE, cluster-aware key reporting included
- **RDB persistence** — bitmaps survive `BGSAVE` and server restarts
- **Container statistics** — `R.STAT` reports cardinality, min/max, and the full array/bitset/run container breakdown for both widths

## Origins

valkey-roaring is based on [redis-roaring](https://github.com/aviggiano/redis-roaring) by Antonio Viggiano and contributors. The command surface and semantics follow redis-roaring, and development continues to track its changes and improvements — applications built against redis-roaring's commands work here unchanged. The binary export/import commands address a long-requested capability ([redis-roaring#141](https://github.com/aviggiano/redis-roaring/issues/141)).

Where redis-roaring wraps the CRoaring C library as a Redis module, valkey-roaring is a ground-up Rust implementation targeting Valkey. The bitmaps come from [roaring-rs](https://github.com/RoaringBitmap/roaring-rs), the RoaringBitmap project's pure-Rust implementation, and the module layer is built on [valkeymodule-rs](https://github.com/valkey-io/valkeymodule-rs), the Valkey project's official Rust SDK. Staying on an all-Rust stack means the module inherits the upstream crate's correctness and performance work, builds with a single `cargo build` and no C toolchain, and picks up improvements to both the bitmap library and the Valkey module ecosystem with a dependency bump.

## Requirements

| Requirement | Version |
|-------------|---------|
| Valkey      | 8.1+    |
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
- `R.SETFULL key` — Set all possible bits (errors if key exists)

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

- `R.OPTIMIZE key` — Optimize internal container storage for better compression
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
"0.4285714285714286"

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
- **Replication:** Every write command propagates verbatim to replicas and to the AOF command stream.
- **AOF:** Supported with the default configuration (`aof-use-rdb-preamble yes`): incremental writes reach the AOF through verbatim propagation, and rewrites use the RDB serialization as the base. The legacy non-preamble AOF mode is not supported (the Valkey Rust SDK does not expose the varargs `EmitAOF` C function needed for its per-type rewrite callback).
- **Registered type names:** `vrroaring` (32-bit), `vroarng64` (64-bit).

### Memory Management

The module sets Rust's global allocator to `ValkeyAlloc`, routing all allocations (bitmaps, buffers, temporary structures) through Valkey's memory tracking. This ensures `INFO MEMORY` accurately reflects module usage.

## Tests

Three layers, all run by CI ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) on every push and pull request, alongside `rustfmt`/`clippy` gates and a unit-layer coverage report. The [benchmark workflow](.github/workflows/benchmark.yml) refreshes the table below whenever performance-relevant code changes. See [CONTRIBUTING.md](CONTRIBUTING.md) for running the gates locally.

**Unit and property tests** (no server needed) — 38 tests covering every
hand-written algorithm:

```bash
cargo test
```

- `nth_absent`, `flip_inclusive`, bit-array codecs, `remove_many_counted`,
  select bounds, u64 reply saturation — including type-boundary edge cases
- All 7 BITOP kernels checked against a naive reference over ~1,100 randomized
  source combinations (both bitmap widths)
- 32-bit / 64-bit parity over 12,000 randomized operations
- Serialization round-trips, plus 800+ corrupted/truncated/garbage inputs fed
  through the `R.IMPORT` deserialization path asserting it never panics

**Integration suite** — 283 assertions against a live Valkey instance:

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
- AOF: replay after restart and rewrite via the RDB preamble
- Dynamic GETKEYS, `BITOP NOT ... last`, duplicate-offset and BITPOS edge cases
- Systematic error coverage: wrong-arity for all 51 commands, WRONGTYPE for
  every key command against a mistyped key, semantic errors (missing keys,
  bad binary, out-of-range values)

**Fuzzing** — three [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz)
targets run 60s each on every push/PR and 10 minutes nightly, with a
persistent corpus cached between runs:

```bash
cargo +nightly fuzz run import_bytes    # untrusted bytes into the R.IMPORT path
cargo +nightly fuzz run parity_ops      # 32-bit vs 64-bit behavioral parity
cargo +nightly fuzz run bitop_kernels   # BITOP kernels vs a naive reference
```

**Performance benchmark** — see [Performance](#performance); CI runs a smoke
subset on every push.

## Performance

Benchmark methodology follows redis-roaring's performance suite: CRoaring's
`census1881` dataset, full client round-trip latency per command against the
dockerized Valkey, compared with the equivalent native commands. The harness
lives in `tests/performance/`.

```bash
bash tests/performance.sh                    # full run, updates this table
PERF_MAX_FILES=5 bash tests/performance.sh   # quick smoke run
```

<!-- BEGIN_PERFORMANCE -->
|               OP |     TIME/OP (us) |     ST.DEV. (us) |
| ---------------- | ---------------- | ---------------- |
|         R.SETBIT |            29.12 |             6.88 |
|       R64.SETBIT |            28.99 |             4.54 |
|           SETBIT |            28.77 |             4.62 |
|         R.GETBIT |            28.86 |             6.09 |
|       R64.GETBIT |            28.78 |             6.29 |
|           GETBIT |            28.63 |             6.04 |
|       R.BITCOUNT |            40.98 |             6.35 |
|     R64.BITCOUNT |            40.97 |             6.48 |
|         BITCOUNT |            48.54 |             7.46 |
|         R.BITPOS |            46.27 |            35.11 |
|       R64.BITPOS |            39.86 |             2.22 |
|           BITPOS |            43.21 |             6.03 |
|      R.BITOP NOT |           116.74 |           308.88 |
|    R64.BITOP NOT |           115.90 |           308.08 |
|        BITOP NOT |           155.53 |            67.22 |
|      R.BITOP AND |            33.26 |            17.18 |
|    R64.BITOP AND |            32.01 |             9.23 |
|        BITOP AND |           152.72 |           141.08 |
|       R.BITOP OR |            36.60 |            22.80 |
|     R64.BITOP OR |            35.55 |            24.60 |
|         BITOP OR |           197.85 |           203.82 |
|      R.BITOP XOR |            36.59 |            31.25 |
|    R64.BITOP XOR |            35.87 |            25.38 |
|        BITOP XOR |           183.51 |           181.87 |
|    R.BITOP ANDOR |            33.50 |            15.68 |
|  R64.BITOP ANDOR |            35.09 |            10.71 |
|      BITOP ANDOR |            28.46 |             0.93 |
|      R.BITOP ONE |            34.75 |            25.99 |
|    R64.BITOP ONE |            35.92 |            29.46 |
|        BITOP ONE |            28.46 |             1.41 |
|            R.MIN |            29.80 |             3.35 |
|          R64.MIN |            28.33 |             1.61 |
|              MIN |            28.26 |             1.26 |
|            R.MAX |            28.18 |             1.63 |
|          R64.MAX |            28.12 |             1.04 |
|              MAX |            30.25 |             7.36 |
<!-- END_PERFORMANCE -->

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

- **Legacy AOF mode**: with `aof-use-rdb-preamble no` (non-default), AOF rewrites cannot include module data — the Valkey Rust SDK does not expose `EmitAOF`. The default preamble mode is fully supported.
- **`R64.SETFULL`** materializes containers for the entire u64 range, which exhausts memory long before completing — inherent to an eager roaring representation (the C module has the same behavior). Avoid it; use `R64.SETRANGE` over the range you actually need.
- **`R.EXPORT` / `R.IMPORT`** binaries cannot be pasted as command arguments; use `valkey-cli -x` / raw output redirection, Lua, or a client library (see [Export / Import](#export--import)).
