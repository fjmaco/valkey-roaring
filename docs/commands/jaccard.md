# R.JACCARD / R64.JACCARD

Returns the Jaccard similarity |A∩B| / |A∪B| of two bitmaps.

| | |
|---|---|
| **Syntax** | `R.JACCARD key1 key2` |
| **64-bit** | `R64.JACCARD key1 key2` |
| **Time complexity** | O(N) |

## Arguments

- **key1, key2** — both keys must exist

## Reply

Bulk string under both RESP2 and RESP3, formatted as redis-roaring formats it:

- `-1` when both bitmaps are empty, `0` when they share nothing, `1` when they are equal;
- otherwise the exact decimal when the ratio has one within nine fractional digits (`0.5`, `0.125`, `0.000000001`);
- otherwise C's `%.17g` of the ratio (`0.33333333333333331`, `3.3333333333333335e-05`).

## Example

```bash
127.0.0.1:6379> R.JACCARD segment:a segment:b
"0.42857142857142855"
```
