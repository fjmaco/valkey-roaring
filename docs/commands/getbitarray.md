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

Bulk string of `0`/`1` characters; `"0"` for an existing empty key. A missing key replies an empty *simple* string, as redis-roaring does.

## Notes

- Refused with `Roaring: range too large: maximum 100000000 elements` when the maximum set bit is at or above 100,000,000 — the reply would be that many bytes. (redis-roaring tries to build the string, and crashes on an `R.SETFULL` key.)

## Example

```bash
127.0.0.1:6379> R.GETBITARRAY k
"0101"
```
