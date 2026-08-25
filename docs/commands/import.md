# R.IMPORT / R64.IMPORT

::: tip valkey-roaring original
`R.IMPORT` does not exist in redis-roaring — it is one of the two
commands this module adds for raw-value interchange: the bitmap deserializes
as CRoaring-portable bytes so any service can consume it directly, without
integer-array round-trips. See the [Export / Import guide](/guide/export-import).
:::

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

Integer: the cardinality after the merge. Malformed payloads are rejected with an error.

## Notes

- Binary can't be pasted as a shell argument — use `valkey-cli -x`, Lua, or a client library (see the [guide](/guide/export-import)).

## Example

```bash
$ valkey-cli -x R.IMPORT users:active < bitmap.bin
(integer) 5
```
