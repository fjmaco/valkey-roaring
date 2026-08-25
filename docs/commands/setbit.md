# R.SETBIT / R64.SETBIT

Sets or clears one bit and returns its previous value.

| | |
|---|---|
| **Syntax** | `R.SETBIT key offset value` |
| **64-bit** | `R64.SETBIT key offset value` |
| **Time complexity** | O(1) |

## Arguments

- **key** — the bitmap key (created if missing)
- **offset** — the bit position (u32 for `R.`, u64 for `R64.`)
- **value** — `1` to set, `0` to clear

## Reply

Integer: the previous bit value (0 or 1).

## Notes

- Setting a bit to `0` on a missing key creates the key with an empty bitmap.

## Example

```bash
127.0.0.1:6379> R.SETBIT k 42 1
(integer) 0
127.0.0.1:6379> R.SETBIT k 42 1
(integer) 1
127.0.0.1:6379> R.SETBIT k 42 0
(integer) 1
```
