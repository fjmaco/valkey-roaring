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
- A bitmap of more than `valkey-roaring.max-reply-elements` values (100,000,000 by default) is refused with `Roaring: range too large: maximum 100000000 elements` (or the configured value) before any reply is written (an `R.SETFULL` key would otherwise stream 2³² values). This is by design: page through such a bitmap with [RANGEINTARRAY](/commands/rangeintarray), up to that many positions per call (see [Configuration](/guide/configuration)).

## Example

```bash
127.0.0.1:6379> R.GETINTARRAY k
1) (integer) 1
2) (integer) 3
3) (integer) 5
```
