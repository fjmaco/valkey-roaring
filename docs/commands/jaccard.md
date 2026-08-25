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

Bulk string holding the similarity as a float (RESP3: a double). Two empty bitmaps reply 0.

## Example

```bash
127.0.0.1:6379> R.JACCARD segment:a segment:b
"0.42857142857142855"
```
