# R.GETINTARRAY / R64.GETINTARRAY

Returns every set bit as a sorted integer array.

| | |
|---|---|
| **Syntax** | `R.GETINTARRAY key` |
| **64-bit** | `R64.GETINTARRAY key` |
| **Time complexity** | O(N) for cardinality N |

## Arguments

- **key** — the bitmap key

## Reply

Array of integers in ascending order; empty array for a missing key. 64-bit values above 2⁶³−1 are replied as decimal strings (RESP integers are signed).

## Notes

- For large bitmaps prefer [RANGEINTARRAY](/commands/rangeintarray) pagination or a binary [EXPORT](/commands/export).

## Example

```bash
127.0.0.1:6379> R.GETINTARRAY k
1) (integer) 1
2) (integer) 3
3) (integer) 5
```
