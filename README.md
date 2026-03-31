# valkey-roaring

Roaring Bitmaps for [Valkey](https://valkey.io/)

## Intro

This project uses the [roaring-rs](https://github.com/RoaringBitmap/roaring-rs) library to implement roaring bitmap commands for [Valkey](https://valkey.io/). Built in Rust with the official [valkey-module-rs](https://github.com/valkey-io/valkeymodule-rs) SDK. Zero C dependencies.

Roaring Bitmaps are compressed data structures that outperform traditional bitmaps on both memory and speed for sparse or clustered integer sets. This module exposes them through **51 commands** across 32-bit (`R.*`) and 64-bit (`R64.*`) variants, including binary export/import in the [CRoaring portable format](https://github.com/RoaringBitmap/CRoaring) for efficient cross-service communication.

This is a Rust rewrite of the C-based [redis-roaring](https://github.com/aviggiano/redis-roaring) module. It adds binary export/import commands ([redis-roaring#141](https://github.com/aviggiano/redis-roaring/issues/141), [redis-roaring#97](https://github.com/aviggiano/redis-roaring/pull/97)) and eliminates the memory safety risks inherent to C module extensions.

### Highlights

- **Memory-safe** — Rust's compiler guarantees eliminate the segfault and buffer-overflow risks of C-based modules
- **Dual range** — 32-bit (0 to 2^32-1) and 64-bit (0 to 2^64-1) bitmap types
- **Binary export** — `R.EXPORT` / `R.IMPORT` serialize to the CRoaring portable format, compatible with Java, Go, Python, C++ libraries
- **8 bitwise operations** — AND, OR, XOR, NOT, ANDOR, DIFF, DIFF1, ONE
- **RDB persistence** — bitmaps survive server restarts via `BGSAVE`
- **No code duplication** — trait-based generic handlers written once, compiled for both bitmap types
- **3x less code** — ~1,400 lines of Rust vs ~4,500 lines of C in redis-roaring

## Dependencies

- [roaring](https://crates.io/crates/roaring) 0.11 — Pure Rust Roaring Bitmap library (no C dependency)
- [valkey-module](https://crates.io/crates/valkey-module) 0.1 — Official Valkey module Rust SDK
- Rust 1.82+ (MSRV)

No C dependencies. No modifications to upstream crates.

## Valkey Version Compatibility

| Requirement | Version |
|-------------|---------|
| Valkey      | 8.1+    |
| Rust        | 1.82+   |
| Docker      | 20.10+  |

## Getting Started

### Docker (recommended)

```bash
docker compose up -d
```

This builds the module from source and starts Valkey 8.1 on port `6379` with `valkey-roaring` loaded.

```bash
docker compose exec valkey valkey-cli
```

### Build from Source

```bash
cd valkey-roaring
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

All commands exist in 32-bit (`R.*`) and 64-bit (`R64.*`) forms. The `R.*` variant accepts `u32` values (0 to 4,294,967,295); `R64.*` accepts `u64` values (0 to 18,446,744,073,709,551,615). Behavior is identical.

### Bit Manipulation

- `R.SETBIT key offset 0|1` — Set or clear a bit (same as [SETBIT](https://valkey.io/commands/setbit))
- `R.GETBIT key offset` — Get bit value (same as [GETBIT](https://valkey.io/commands/getbit))
- `R.GETBITS key offset [offset ...]` — Get multiple bit values at once
- `R.CLEARBITS key offset [offset ...]` — Clear multiple bits, returns count actually cleared
- `R.CLEAR key` — Reset bitmap to empty, returns previous cardinality

### Bulk Set/Get

- `R.SETINTARRAY key val [val ...]` — Replace bitmap with integer set
- `R.GETINTARRAY key` — Get all set bits as sorted integer array
- `R.APPENDINTARRAY key val [val ...]` — Add integers to bitmap
- `R.DELETEINTARRAY key val [val ...]` — Remove integers from bitmap
- `R.RANGEINTARRAY key start end` — Get set bits in [start, end] range (supports pagination)

### Bit Array

- `R.SETBITARRAY key "010110..."` — Create bitmap from ASCII bit string
- `R.GETBITARRAY key` — Get bitmap as ASCII bit string

### Range and Fill

- `R.SETRANGE key start end` — Set all bits in [start, end]
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

**CONTAINS modes:** `NONE` (any overlap), `ALL` (subset), `ALL_STRICT` (proper subset), `EQ` (equal).

### Bitwise Operations

```
R.BITOP <op> destkey srckey [srckey ...]
```

Same as [BITOP](https://valkey.io/commands/bitop) with extended operations:

| Operation | Semantics |
|-----------|-----------|
| `AND`     | Intersection of all sources |
| `OR`      | Union of all sources |
| `XOR`     | Symmetric difference |
| `NOT`     | Complement of single source (bits in [0, max]) |
| `ANDOR`   | `(src[1] \| src[2] \| ...) & src[0]` |
| `DIFF`    | `src[0] - src[1] - src[2] - ...` |
| `DIFF1`   | `(src[1] \| src[2] \| ...) - src[0]` |
| `ONE`     | Bits present in exactly one source |

All BITOP operations return the cardinality of the result.

### Export / Import

- `R.EXPORT key` — Serialize to CRoaring portable binary format
- `R.IMPORT key binary` — Deserialize and OR-merge into key, returns cardinality after import

The binary output of `R.EXPORT` is compatible with any [CRoaring-compatible library](#croaring-compatible-libraries) (Java, Go, Python, C++, Rust). This is the recommended way to transfer bitmaps between services.

Binary data must be passed via Lua or a client library (`valkey-cli` drops null bytes):

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

# create a bitmap from a range
127.0.0.1:6379> R.SETRANGE range_test 1 100
OK

# get all numbers as an integer array
127.0.0.1:6379> R.GETINTARRAY range_test
  1) (integer) 1
  2) (integer) 2
  ...
100) (integer) 100

# paginate with RANGEINTARRAY
127.0.0.1:6379> R.RANGEINTARRAY range_test 50 60
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
"cardinality: 2\nmin: 42\nmax: 123\nserialized_bytes: 22\n..."

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
  commands.rs         22 generic command handlers
  commands_bitop.rs   BITOP dispatch + 8 sub-operations
  error.rs            Error constants
  parse.rs            Argument parsing
```

Every command handler is a single generic function parameterized by the `RoaringType` trait. At module load, it is instantiated twice via monomorphization:

```rust
fn handle_setbit<T: RoaringType>(ctx, args, vtype) -> ValkeyResult { ... }

// Registered as:
["R.SETBIT",   r_setbit,   ...]   // T = RoaringBitmap (u32)
["R64.SETBIT", r64_setbit, ...]   // T = RoaringTreemap (u64)
```

This eliminates the ~2,300 lines of duplicated C code found in redis-roaring's `r_32.c` / `r_64.c`.

### Persistence

- **RDB:** Bitmaps serialize via the CRoaring portable binary format. Data survives `BGSAVE` and server restarts.
- **Registered type names:** `vrroaring` (32-bit), `vroarng64` (64-bit).
- **AOF:** Not currently supported (the Valkey Rust SDK does not expose the varargs `EmitAOF` C function).

### Memory Management

The module sets Rust's global allocator to `ValkeyAlloc`, routing all allocations (bitmaps, buffers, temporary structures) through Valkey's memory tracking. This ensures `INFO MEMORY` accurately reflects module usage.

## Tests

The integration test suite runs **102 assertions** against a live Valkey instance:

```bash
# From the repository root (requires running docker compose)
bash tests/integration.sh
```

Coverage includes:

- Every command for both 32-bit and 64-bit types
- All 8 BITOP sub-operations with correctness checks
- CONTAINS with all 4 modes (NONE, ALL, ALL_STRICT, EQ)
- EXPORT/IMPORT binary round-trip via Lua
- RDB persistence across server restart
- Error handling (wrong type, wrong arity, nonexistent keys)

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

- **AOF rewrite** not supported (Valkey Rust SDK limitation)
- **`R64.SETFULL`** allocates the full u64 range which is impractical — matches redis-roaring behavior
- **64-bit `R.STAT`** shows basic stats only (cardinality, min, max, serialized_size) — no container breakdown because `RoaringTreemap.map` is private in the roaring crate
- **`R.EXPORT` / `R.IMPORT`** require a client library or Lua — `valkey-cli` drops null bytes from binary data

