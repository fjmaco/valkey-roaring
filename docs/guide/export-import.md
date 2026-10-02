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

`valkey-cli`'s raw output and `-x` flag are binary-safe:

```bash
valkey-cli R.EXPORT source > bitmap.bin          # raw reply → file
valkey-cli -x R.IMPORT destination < bitmap.bin  # stdin → last argument
```

Raw binary can't be pasted as a command argument, but its Base64 text can.
Add `BASE64` to either command:

```bash
$ valkey-cli R.EXPORT users:active BASE64
"OjAAAAEAAAAAAAEAEAAAACoAZAA="
$ valkey-cli R.IMPORT users:copy OjAAAAEAAAAAAAEAEAAAACoAZAA= BASE64
(integer) 2

# or in one step
valkey-cli R.IMPORT destination "$(valkey-cli R.EXPORT source BASE64)" BASE64
```

The text is standard Base64 (RFC 4648 alphabet, `=` padding), so
`base64 -d` or any language's decoder turns it back into the blob:

```bash
valkey-cli R.EXPORT source BASE64 | base64 -d > bitmap.bin
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
  blobs are as small as the data allows. They are also canonical: one set
  always exports the same bytes, and the same Base64 text.
- Malformed input to `R.IMPORT` is rejected with an error — the
  deserialization path is fuzz-tested against corrupted, truncated, and
  garbage bytes.
- Base64 text is decoded strictly: no whitespace or line breaks, no
  URL-safe alphabet, padding required. Text that is not exactly what
  `R.EXPORT ... BASE64` would produce for some blob is rejected with
  `ERR bad binary data for roaring`.
- An `R.IMPORT ... BASE64` replicates and reaches the AOF exactly as sent,
  and replays the same way.
