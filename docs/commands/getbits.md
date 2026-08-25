# R.GETBITS / R64.GETBITS

Returns the values of several bits in one call.

| | |
|---|---|
| **Syntax** | `R.GETBITS key offset [offset ...]` |
| **64-bit** | `R64.GETBITS key offset [offset ...]` |
| **Time complexity** | O(K) for K offsets |

## Arguments

- **key** — the bitmap key
- **offset ...** — one or more bit positions

## Reply

Array of integers (0 or 1) in argument order. A missing key replies an **empty array**.

## Example

```bash
127.0.0.1:6379> R.SETINTARRAY k 1 3 5
OK
127.0.0.1:6379> R.GETBITS k 1 2 3
1) (integer) 1
2) (integer) 0
3) (integer) 1
```
