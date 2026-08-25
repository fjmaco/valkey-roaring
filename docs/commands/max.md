# R.MAX / R64.MAX

Returns the largest set bit.

| | |
|---|---|
| **Syntax** | `R.MAX key` |
| **64-bit** | `R64.MAX key` |
| **Time complexity** | O(1) |

## Arguments

- **key** — the bitmap key

## Reply

Integer; −1 for a missing or empty bitmap. 64-bit values above 2⁶³−1 are replied as decimal strings.

## Example

```bash
127.0.0.1:6379> R.MAX k
(integer) 30
```
