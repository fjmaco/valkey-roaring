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

Bulk string of `0`/`1` characters; `"0"` for an existing empty key. A missing key replies an empty *simple* string.

## Notes

- Refused with `Roaring: range too large: maximum 100000000 elements` when the maximum set bit is at or above `valkey-roaring.max-reply-elements` (100,000,000 by default; the refusal names the configured value) — the reply would be that many bytes. This includes an `R.SETFULL` key.

## Example

```bash
127.0.0.1:6379> R.GETBITARRAY k
"0101"
```
