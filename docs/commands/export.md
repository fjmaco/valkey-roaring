# R.EXPORT / R64.EXPORT

Serializes the bitmap to the CRoaring portable binary format, optionally as
Base64 text.

| | |
|---|---|
| **Syntax** | `R.EXPORT key [BASE64]` |
| **64-bit** | `R64.EXPORT key [BASE64]` |
| **Time complexity** | O(N) |

## Arguments

- **key** — must exist
- **BASE64** — optional: reply with the blob as Base64 text (standard
  alphabet, with `=` padding) instead of raw bytes. Case-sensitive; any
  other token is `ERR syntax error`.

## Reply

Bulk string with the raw binary payload, or with its Base64 text when
`BASE64` is given.

## Notes

- The payload deserializes in any Roaring library — see the [Export / Import guide](/guide/export-import) for shell, Lua, and Python recipes.
- Storage is optimized before serializing, so the blob is as small as the data allows.
- The blob is canonical (one set, one byte sequence), and so is its Base64 text.
- The key is checked before the token: a missing key replies `Roaring: key does not exist` and a key of another type `WRONGTYPE`.

## Example

```bash
$ valkey-cli R.EXPORT users:active > bitmap.bin
$ valkey-cli R.EXPORT users:active BASE64
"OjAAAAEAAAAAAAEAEAAAACoAZAA="
```
