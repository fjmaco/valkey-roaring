# R.CLEARBITS / R64.CLEARBITS

Clears several bits in one call.

| | |
|---|---|
| **Syntax** | `R.CLEARBITS key offset [offset ...] [COUNT]` |
| **64-bit** | `R64.CLEARBITS key offset [offset ...] [COUNT]` |
| **Time complexity** | O(K) for K offsets |

## Arguments

- **key** — the bitmap key
- **offset ...** — bit positions to clear
- **COUNT** — optional trailing flag: reply with the number of bits actually cleared

## Reply

Simple string `OK` — or, with the `COUNT` flag, an integer counting bits that were set and are now cleared (duplicates count once). A missing key replies null and nothing happens.

## Example

```bash
127.0.0.1:6379> R.SETINTARRAY k 1 2 3
OK
127.0.0.1:6379> R.CLEARBITS k 1
OK
127.0.0.1:6379> R.CLEARBITS k 2 3 99 COUNT
(integer) 2
```
