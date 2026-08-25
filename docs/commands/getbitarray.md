# R.GETBITARRAY / R64.GETBITARRAY

Returns the bitmap as an ASCII bit string of length max+1.

| | |
|---|---|
| **Syntax** | `R.GETBITARRAY key` |
| **64-bit** | `R64.GETBITARRAY key` |
| **Time complexity** | O(max) |

## Arguments

- **key** — the bitmap key

## Reply

Bulk string of `0`/`1` characters; empty string for a missing or empty key.

## Notes

- Refused with an error when the maximum set bit is at or above 100,000,000 — the reply would be that many bytes.

## Example

```bash
127.0.0.1:6379> R.GETBITARRAY k
"0101"
```
