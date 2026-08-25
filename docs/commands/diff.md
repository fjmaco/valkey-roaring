# R.DIFF / R64.DIFF

Stores the set difference key1 − key2 into dest.

| | |
|---|---|
| **Syntax** | `R.DIFF dest key1 key2` |
| **64-bit** | `R64.DIFF dest key1 key2` |
| **Time complexity** | O(N) |

## Arguments

- **dest** — destination key (overwritten)
- **key1, key2** — both source keys must exist

## Reply

Simple string `OK`.

## Notes

- For variadic differences use [`R.BITOP DIFF`](/commands/bitop).

## Example

```bash
127.0.0.1:6379> R.DIFF only_a segment:a segment:b
OK
```
