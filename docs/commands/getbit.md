# R.GETBIT / R64.GETBIT

Returns the value of one bit.

| | |
|---|---|
| **Syntax** | `R.GETBIT key offset` |
| **64-bit** | `R64.GETBIT key offset` |
| **Time complexity** | O(1) |

## Arguments

- **key** — the bitmap key
- **offset** — the bit position

## Reply

Integer: 0 or 1. A missing key reads as all-zero.

## Example

```bash
127.0.0.1:6379> R.GETBIT k 42
(integer) 1
127.0.0.1:6379> R.GETBIT missing 42
(integer) 0
```
