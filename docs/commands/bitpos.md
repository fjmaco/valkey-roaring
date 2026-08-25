# R.BITPOS / R64.BITPOS

Returns the position of the first set (`bit`=1) or first unset (`bit`=0) bit.

| | |
|---|---|
| **Syntax** | `R.BITPOS key bit` |
| **64-bit** | `R64.BITPOS key bit` |
| **Time complexity** | O(prefix) |

## Arguments

- **key** — the bitmap key
- **bit** — `1` or `0`

## Reply

Integer position. Missing key: −1 for `bit`=1, 0 for `bit`=0. A full 32-bit bitmap has no unset bit and replies −1.

## Example

```bash
127.0.0.1:6379> R.SETINTARRAY k 0 1 2 5
OK
127.0.0.1:6379> R.BITPOS k 1
(integer) 0
127.0.0.1:6379> R.BITPOS k 0
(integer) 3
```
