# R.IMPORT / R64.IMPORT

Deserializes a CRoaring-portable payload and OR-merges it into key.

| | |
|---|---|
| **Syntax** | `R.IMPORT key binary` |
| **64-bit** | `R64.IMPORT key binary` |
| **Time complexity** | O(N) |

## Arguments

- **key** — created if missing
- **binary** — the serialized bitmap

## Reply

Integer: the cardinality after the merge. A payload that is not exactly one valid bitmap is rejected with `ERR bad binary data for roaring` and nothing is changed: malformed or truncated containers, trailing bytes after a complete bitmap, and (64-bit) sub-bitmaps whose high 32-bit words do not strictly increase — CRoaring rejects those too, where roaring-rs alone would keep the last of a repeated word and silently drop the others' values. Empty sub-bitmaps are valid and ignored. Commands replayed from the AOF or received from a primary are decoded with 1.1.1's rules instead (trailing bytes ignored, the last of a repeated high word kept), so data written through 1.1.1 survives an upgrade; see [Upgrading from 1.1.1](/guide/persistence-and-replication#upgrading-from-1-1-1).

## Notes

- Binary can't be pasted as a shell argument — use `valkey-cli -x`, Lua, or a client library (see the [guide](/guide/export-import)).

## Example

```bash
$ valkey-cli -x R.IMPORT users:active < bitmap.bin
(integer) 5
```
