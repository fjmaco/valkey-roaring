# R.IMPORT / R64.IMPORT

Deserializes a CRoaring-portable payload, raw or as Base64 text, and
OR-merges it into key.

| | |
|---|---|
| **Syntax** | `R.IMPORT key binary [BASE64]` |
| **64-bit** | `R64.IMPORT key binary [BASE64]` |
| **Time complexity** | O(N) |

## Arguments

- **key** — created if missing
- **binary** — the serialized bitmap, or its Base64 text with `BASE64`
- **BASE64** — optional: `binary` is Base64 text (standard alphabet, with
  `=` padding), as `EXPORT key BASE64` replies it. Case-sensitive; any
  other token is `ERR syntax error`.

## Reply

Integer: the cardinality after the merge. A payload that is not exactly one valid bitmap is rejected with `ERR bad binary data for roaring` and nothing is changed: malformed or truncated containers, trailing bytes after a complete bitmap, and (64-bit) sub-bitmaps whose high 32-bit words do not strictly increase — CRoaring rejects those too, where roaring-rs alone would keep the last of a repeated word and silently drop the others' values. Empty sub-bitmaps are valid and ignored. Commands replayed from the AOF or received from a primary are decoded with 1.1.1's rules instead (trailing bytes ignored, the last of a repeated high word kept), so data written through 1.1.1 survives an upgrade; see [Upgrading from 1.1.1](/guide/persistence-and-replication#upgrading-from-1-1-1).

Base64 text is decoded strictly, so each blob has exactly one text: its
length is a multiple of 4, `=` appears only as one or two final padding
characters, the bits the padding leaves unused are zero, and nothing
outside the alphabet is accepted (no whitespace or line breaks, no URL-safe
`-` / `_`). Text that breaks any of these is `ERR bad binary data for
roaring`, like a malformed blob.

## Notes

- Raw binary can't be pasted as a shell argument — use `BASE64`, `valkey-cli -x`, Lua, or a client library (see the [guide](/guide/export-import)).
- The command replicates and reaches the AOF exactly as sent, `BASE64` text included.

## Example

```bash
$ valkey-cli -x R.IMPORT users:active < bitmap.bin
(integer) 5
$ valkey-cli R.IMPORT users:copy OjAAAAEAAAAAAAEAEAAAACoAZAA= BASE64
(integer) 2
```
