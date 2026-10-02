# R.RANGEINTARRAY / R64.RANGEINTARRAY

Paginates the sorted value array: returns the elements at 0-based positions `start` through `end`.

| | |
|---|---|
| **Syntax** | `R.RANGEINTARRAY key start end` |
| **64-bit** | `R64.RANGEINTARRAY key start end` |
| **Time complexity** | O(K log N) for a K-wide window |

## Arguments

- **key** — the bitmap key
- **start** — 0-based index of the first element to return
- **end** — 0-based index of the last element to return (inclusive)

## Reply

Array of integers; truncated at the cardinality; empty for a missing key or an inverted range.

## Notes

- `start`/`end` are **positions**, not values — this is the pagination companion to `GETINTARRAY`.
- The window may span at most 100,000,000 positions; wider windows are rejected with `Roaring: range too large: maximum 100000000 elements`.
- One exception: the full-width request (`0` to 4294967295, or to 18446744073709551615 for R64) lists the whole bitmap, and is refused only when the bitmap holds more than 100,000,000 values.
- 64-bit positions are full unsigned 64-bit numbers: a window past the cardinality is simply empty.

## Example

```bash
127.0.0.1:6379> R.SETINTARRAY k 5 10 15 20 25 30
OK
127.0.0.1:6379> R.RANGEINTARRAY k 1 3
1) (integer) 10
2) (integer) 15
3) (integer) 20
127.0.0.1:6379> R.RANGEINTARRAY k 4 100
1) (integer) 25
2) (integer) 30
```
