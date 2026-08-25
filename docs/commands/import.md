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

Integer: the cardinality after the merge. Malformed payloads are rejected with an error.

## Notes

- Binary can't be pasted as a shell argument — use `valkey-cli -x`, Lua, or a client library (see the [guide](/guide/export-import)).

## Example

```bash
$ valkey-cli -x R.IMPORT users:active < bitmap.bin
(integer) 5
```
