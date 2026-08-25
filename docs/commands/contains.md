# R.CONTAINS / R64.CONTAINS

Tests the set relationship between two bitmaps.

| | |
|---|---|
| **Syntax** | `R.CONTAINS key1 key2 [mode]` |
| **64-bit** | `R64.CONTAINS key1 key2 [mode]` |
| **Time complexity** | O(N) |

## Arguments

- **key1, key2** — both keys must exist
- **mode** — optional: `ALL` (key2 ⊆ key1), `ALL_STRICT` (proper subset), `EQ` (equal). Without a mode: any overlap.

## Reply

Integer 0 or 1.

## Notes

- The default overlap check has no token — an explicit `NONE` argument is rejected (redis-roaring parity).

## Example

```bash
127.0.0.1:6379> R.CONTAINS segment:a segment:b
(integer) 1
127.0.0.1:6379> R.CONTAINS segment:a segment:b ALL
(integer) 0
```
