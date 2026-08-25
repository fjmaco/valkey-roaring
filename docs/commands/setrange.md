# R.SETRANGE / R64.SETRANGE

Sets every bit in the **end-exclusive** range [start, end).

| | |
|---|---|
| **Syntax** | `R.SETRANGE key start end` |
| **64-bit** | `R64.SETRANGE key start end` |
| **Time complexity** | O(end - start) |

## Arguments

- **key** — the bitmap key (created if missing)
- **start** — first bit to set
- **end** — first bit NOT set — must be >= start

## Reply

Simple string `OK`.

## Notes

- End-exclusive, matching redis-roaring and CRoaring `add_range`: `R.SETRANGE k 5 8` sets bits 5, 6, 7.
- `end < start` is an error; `end == start` sets nothing (but creates the key).

## Example

```bash
127.0.0.1:6379> R.SETRANGE k 5 8
OK
127.0.0.1:6379> R.GETINTARRAY k
1) (integer) 5
2) (integer) 6
3) (integer) 7
```
