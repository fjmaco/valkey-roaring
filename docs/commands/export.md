# R.EXPORT / R64.EXPORT

::: tip valkey-roaring original
`R.EXPORT` does not exist in redis-roaring — it is one of the two
commands this module adds for raw-value interchange: the bitmap serializes
as CRoaring-portable bytes so any service can consume it directly, without
integer-array round-trips. See the [Export / Import guide](/guide/export-import).
:::

Serializes the bitmap to the CRoaring portable binary format.

| | |
|---|---|
| **Syntax** | `R.EXPORT key` |
| **64-bit** | `R64.EXPORT key` |
| **Time complexity** | O(N) |

## Arguments

- **key** — must exist

## Reply

Bulk string with the raw binary payload.

## Notes

- The payload deserializes in any Roaring library — see the [Export / Import guide](/guide/export-import) for shell, Lua, and Python recipes.
- Storage is optimized before serializing, so the blob is as small as the data allows.

## Example

```bash
$ valkey-cli R.EXPORT users:active > bitmap.bin
```
