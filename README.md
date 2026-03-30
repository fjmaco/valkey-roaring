# valkey-roaring

A [Valkey](https://valkey.io/) module that adds [Roaring Bitmap](https://roaringbitmap.org/) data structures as native server-side types. Built in Rust with the official [valkey-module-rs](https://github.com/valkey-io/valkeymodule-rs) SDK and the [roaring](https://crates.io/crates/roaring) crate. Zero C dependencies.

Roaring Bitmaps are compressed data structures that outperform traditional bitmaps on both memory and speed for sparse or clustered integer sets. This module exposes them through **51 commands** across 32-bit (`R.*`) and 64-bit (`R64.*`) variants, including binary export/import for efficient cross-service communication.

## Highlights

- **Memory-safe** — Rust's compiler guarantees eliminate the segfault and buffer-overflow risks of C-based modules
- **Dual range** — 32-bit (0 to 2^32-1) and 64-bit (0 to 2^64-1) bitmap types
- **Binary export** — `R.EXPORT` / `R.IMPORT` serialize to the [CRoaring portable format](https://github.com/RoaringBitmap/CRoaring), compatible with Java, Go, Python, C++ libraries
- **8 bitwise operations** — AND, OR, XOR, NOT, ANDOR, DIFF, ORNOT, ONE
- **RDB persistence** — bitmaps survive server restarts
- **No code duplication** — trait-based generic handlers written once, compiled for both bitmap types

---

## Getting Started

### Docker (recommended)

From the repository root:

```bash
docker compose up -d
```

This builds the module from source and starts Valkey 8.1 on port `6379` with `valkey-roaring` loaded.

```bash
docker compose exec valkey valkey-cli R.SETBIT users:active 42 1
docker compose exec valkey valkey-cli R.BITCOUNT users:active
# (integer) 1
```

### Build from Source

**Prerequisites:** Rust 1.82+, libclang-dev

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
valkey-cli R.EXPORT test          # (binary blob)
```

---

## Command Reference

All commands exist in 32-bit (`R.*`) and 64-bit (`R64.*`) forms. The `R.*` variant accepts `u32` offsets; `R64.*` accepts `u64` offsets. Behavior is identical.

### Bit Manipulation

| Command | Description | Time Complexity | Returns |
|---------|-------------|-----------------|---------|
| `R.SETBIT key offset 0\|1` | Set or clear a bit | O(1) | Previous bit value |
| `R.GETBIT key offset` | Get bit value | O(1) | 0 or 1 |
| `R.GETBITS key off [off ...]` | Get multiple bit values | O(k) | Array of 0/1 |
| `R.CLEARBITS key off [off ...]` | Clear multiple bits | O(k) | Count actually cleared |
| `R.CLEAR key` | Reset bitmap to empty | O(1) | Previous cardinality |

### Bulk Set/Get

| Command | Description | Returns |
|---------|-------------|---------|
| `R.SETINTARRAY key val [val ...]` | Replace bitmap with integer set | OK |
| `R.GETINTARRAY key` | Get all set bits as sorted array | Array |
| `R.APPENDINTARRAY key val [val ...]` | Add integers to bitmap | OK |
| `R.DELETEINTARRAY key val [val ...]` | Remove integers from bitmap | OK |
| `R.RANGEINTARRAY key start end` | Get set bits in \[start, end\] | Array |

### Bit Array

| Command | Description | Returns |
|---------|-------------|---------|
| `R.SETBITARRAY key "010110..."` | Create bitmap from ASCII bit string | OK |
| `R.GETBITARRAY key` | Get bitmap as ASCII bit string | String |

### Range and Fill

| Command | Description | Returns |
|---------|-------------|---------|
| `R.SETRANGE key start end` | Set all bits in \[start, end\] | OK |
| `R.SETFULL key` | Set all possible bits (errors if key exists) | OK |

### Aggregation

| Command | Description | Returns |
|---------|-------------|---------|
| `R.BITCOUNT key` | Cardinality (number of set bits) | Integer |
| `R.BITPOS key 0\|1` | Position of first set (1) or unset (0) bit | Integer or -1 |
| `R.MIN key` | Smallest set bit | Integer or -1 |
| `R.MAX key` | Largest set bit | Integer or -1 |

### Set Operations

| Command | Description | Returns |
|---------|-------------|---------|
| `R.CONTAINS key1 key2 [mode]` | Check relationship between bitmaps | 0 or 1 |
| `R.JACCARD key1 key2` | Jaccard similarity index | Float |
| `R.DIFF dest key1 key2` | Store `key1 - key2` in dest | OK |

**CONTAINS modes:** `NONE` (any overlap), `ALL` (subset), `ALL_STRICT` (proper subset), `EQ` (equal).

### Bitwise Operations

```
R.BITOP <op> destkey srckey [srckey ...]
```

| Operation | Semantics | Returns |
|-----------|-----------|---------|
| `AND` | Intersection of all sources | Result cardinality |
| `OR` | Union of all sources | Result cardinality |
| `XOR` | Symmetric difference | Result cardinality |
| `NOT` | Complement of single source (bits in \[0, max\]) | Result cardinality |
| `ANDOR` | `(src[1] \| src[2] \| ...) & src[0]` | Result cardinality |
| `DIFF` | `src[0] - src[1] - src[2] - ...` | Result cardinality |
| `DIFF1` | `(src[1] \| src[2] \| ...) - src[0]` | Result cardinality |
| `ONE` | Bits present in exactly one source | Result cardinality |

### Export / Import

| Command | Description | Returns |
|---------|-------------|---------|
| `R.EXPORT key` | Serialize to CRoaring portable binary | Binary buffer |
| `R.IMPORT key binary` | Deserialize and OR-merge into key | Cardinality after import |

Binary data must be passed via Lua or a client library (valkey-cli drops null bytes):

```lua
local data = redis.call('R.EXPORT', 'source')
redis.call('R.IMPORT', 'destination', data)
```

### Maintenance

| Command | Description | Returns |
|---------|-------------|---------|
| `R.OPTIMIZE key` | Optimize internal container storage | OK |
| `R.STAT key [TEXT\|JSON]` | Container statistics (works for both R.\* and R64.\* keys) | String |

---

## Architecture

```
src/
 lib.rs              Module entry, type registration, 51 command wrappers
 bitmap_type.rs      RoaringType trait (abstracts u32 vs u64)
 bitmap32.rs         impl RoaringType for RoaringBitmap
 bitmap64.rs         impl RoaringType for RoaringTreemap
 commands.rs         22 generic command handlers
 commands_bitop.rs   BITOP dispatch + 8 sub-operations
 error.rs            Error constants
 parse.rs            Argument parsing
```

### Generic Design

Every command handler is a single generic function parameterized by the `RoaringType` trait. At module load, it is instantiated twice:

```rust
fn handle_setbit<T: RoaringType>(ctx, args, vtype) -> ValkeyResult { ... }

// Registered as:
["R.SETBIT",   r_setbit,   ...]   // T = RoaringBitmap (u32)
["R64.SETBIT", r64_setbit, ...]   // T = RoaringTreemap (u64)
```

### Persistence

- **RDB:** Bitmaps serialize via the CRoaring portable binary format. Data survives `BGSAVE` and server restarts.
- **Registered type names:** `vrroaring` (32-bit), `vroarng64` (64-bit).
- **AOF:** Not currently supported (the Valkey Rust SDK does not expose the varargs `EmitAOF` C function).

---

## Testing

The integration test suite runs 102 assertions against a live Valkey instance:

```bash
# From the repository root (requires running docker compose)
bash tests/integration.sh
```

Coverage includes:
- Every command for both 32-bit and 64-bit types
- All 8 BITOP sub-operations with correctness checks
- CONTAINS with all 4 modes
- EXPORT/IMPORT binary round-trip via Lua
- RDB persistence across server restart
- Error handling (wrong type, wrong arity, nonexistent keys)

---

## Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| [valkey-module](https://crates.io/crates/valkey-module) | 0.1 | Valkey module SDK |
| [roaring](https://crates.io/crates/roaring) | 0.11 | Roaring Bitmap library (pure Rust) |

No C dependencies. No modifications to upstream crates.

## Compatibility

| Requirement | Version |
|-------------|---------|
| Valkey | 8.1+ |
| Rust | 1.82+ (MSRV) |
| Docker | 20.10+ (for docker compose) |

### CRoaring-compatible libraries (for EXPORT/IMPORT interop)

| Language | Library |
|----------|---------|
| Java | [RoaringBitmap](https://github.com/RoaringBitmap/RoaringBitmap) |
| Go | [roaring](https://github.com/RoaringBitmap/roaring) |
| Python | [pyroaring](https://github.com/Ezibenroc/PyRoaringBitMap) |
| C/C++ | [CRoaring](https://github.com/RoaringBitmap/CRoaring) |
| Rust | [roaring-rs](https://github.com/RoaringBitmap/roaring-rs) |

---

## Background

This module is a Rust rewrite of the C-based [redis-roaring](https://github.com/aviggiano/redis-roaring) module. It adds the long-requested binary export/import commands ([#141](https://github.com/aviggiano/redis-roaring/issues/141), [#97](https://github.com/aviggiano/redis-roaring/pull/97)) and eliminates the memory safety risks inherent to C module extensions.

## License

MIT
