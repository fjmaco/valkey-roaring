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
- The window may span at most 100,000,000 positions; wider windows are rejected with an error.

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
