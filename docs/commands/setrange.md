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

- End-exclusive, like CRoaring's `add_range`: `R.SETRANGE k 5 8` sets bits 5, 6, 7.
- `end < start` is an error: `ERR invalid end: must be >= start` (R), `ERR invalid end: must >= start` (R64; the wording differs by width). `end == start` sets nothing (but creates the key).
- One call may set at most `valkey-roaring.max-write-values` values, 2³⁸ by default; a wider range is refused with `Roaring: range too large: maximum 274877906944 elements` (or the configured value). With `maxmemory` set, a range whose estimated size would push used memory past it is refused with `OOM command not allowed when used memory > 'maxmemory'.` Either way nothing is created (see [limits](/commands/#limits)).

## Example

```bash
127.0.0.1:6379> R.SETRANGE k 5 8
OK
127.0.0.1:6379> R.GETINTARRAY k
1) (integer) 5
2) (integer) 6
3) (integer) 7
```
