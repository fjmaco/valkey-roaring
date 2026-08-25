# R.BITCOUNT / R64.BITCOUNT

Returns the cardinality (number of set bits).

| | |
|---|---|
| **Syntax** | `R.BITCOUNT key` |
| **64-bit** | `R64.BITCOUNT key` |
| **Time complexity** | O(1) |

## Arguments

- **key** — the bitmap key

## Reply

Integer; 0 for a missing key.

## Notes

- Roaring caches per-container cardinalities, so this is a lookup, not a scan.

## Example

```bash
127.0.0.1:6379> R.BITCOUNT k
(integer) 3
```
