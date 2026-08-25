# R.MIN / R64.MIN

Returns the smallest set bit.

| | |
|---|---|
| **Syntax** | `R.MIN key` |
| **64-bit** | `R64.MIN key` |
| **Time complexity** | O(1) |

## Arguments

- **key** — the bitmap key

## Reply

Integer; −1 for a missing or empty bitmap.

## Example

```bash
127.0.0.1:6379> R.MIN k
(integer) 5
```
