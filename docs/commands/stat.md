# R.STAT

Returns statistics about a bitmap: type, cardinality, min/max, and the full
array/bitset/run container breakdown. Shared between both widths — it
auto-detects whether the key holds a 32-bit or 64-bit bitmap.

| | |
|---|---|
| **Syntax** | `R.STAT key [TEXT\|JSON]` |
| **Time complexity** | O(containers) |

## Arguments

- **key** — the bitmap key (either width)
- **format** — `TEXT` (default) or `JSON`

## Reply

Bulk string in the requested format; null for a missing key.

## Example

```bash
127.0.0.1:6379> R.STAT k JSON
"{\"type\":\"bitmap\",\"cardinality\":\"3\",...}"
```
