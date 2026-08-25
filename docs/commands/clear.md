# R.CLEAR / R64.CLEAR

Empties the bitmap, keeping the key.

| | |
|---|---|
| **Syntax** | `R.CLEAR key` |
| **64-bit** | `R64.CLEAR key` |
| **Time complexity** | O(1) |

## Arguments

- **key** — the bitmap key

## Reply

Integer: the cardinality before clearing. A missing key replies null.

## Example

```bash
127.0.0.1:6379> R.SETINTARRAY k 1 2 3
OK
127.0.0.1:6379> R.CLEAR k
(integer) 3
127.0.0.1:6379> R.BITCOUNT k
(integer) 0
```
