# Export / Import

`R.EXPORT` serializes a bitmap to the
[CRoaring portable format](https://github.com/RoaringBitmap/RoaringFormatSpec)
— the interchange standard implemented by every major Roaring library — and
`R.IMPORT` reads it back, OR-merging into the destination key. This is the
module's signature capability: a compressed set leaves Valkey as a small
binary blob and deserializes natively anywhere.

```
Valkey (R.EXPORT)  →  binary blob  →  Java / Go / Python / C++ / Rust service
```

For sparse or clustered sets the blob is typically **10–40× smaller** than
the equivalent integer array, and the receiving side pays no parsing cost.

## From a shell

`valkey-cli`'s raw output and `-x` flag are binary-safe; pasting binary as a
command argument is not:

```bash
valkey-cli R.EXPORT source > bitmap.bin          # raw reply → file
valkey-cli -x R.IMPORT destination < bitmap.bin  # stdin → last argument
```

## From Lua

```lua
local data = redis.call('R.EXPORT', 'source')
redis.call('R.IMPORT', 'destination', data)
```

## From Python (pyroaring)

```python
from pyroaring import BitMap
import valkey

client = valkey.Valkey()
blob = client.execute_command("R.EXPORT", "users:active")
bm = BitMap.deserialize(blob)          # a real CRoaring bitmap

bm.add(999)
client.execute_command("R.IMPORT", "users:active", BitMap.serialize(bm))
```

The 64-bit variant round-trips the same way with `BitMap64`, including
values above 2⁶³.

## Compatible libraries

| Language | Library |
|----------|---------|
| Java     | [RoaringBitmap](https://github.com/RoaringBitmap/RoaringBitmap) |
| Go       | [roaring](https://github.com/RoaringBitmap/roaring) |
| Python   | [pyroaring](https://github.com/Ezibenroc/PyRoaringBitMap) |
| C/C++    | [CRoaring](https://github.com/RoaringBitmap/CRoaring) |
| Rust     | [roaring-rs](https://github.com/RoaringBitmap/roaring-rs) |

Byte-compatibility in both directions and both widths is verified against
CRoaring itself as part of the project's validation suite.

## Semantics worth knowing

- `R.IMPORT` **OR-merges** into an existing key (and creates it when
  missing); it replies with the cardinality after the merge.
- `R.EXPORT` optimizes container storage before serializing, so exported
  blobs are as small as the data allows.
- Malformed input to `R.IMPORT` is rejected with an error — the
  deserialization path is fuzz-tested against corrupted, truncated, and
  garbage bytes.
