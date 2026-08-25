# R.SETFULL / R64.SETFULL

Creates the key with every possible bit set.

| | |
|---|---|
| **Syntax** | `R.SETFULL key` |
| **64-bit** | `R64.SETFULL key` |
| **Time complexity** | O(1) for 32-bit |

## Arguments

- **key** — must not already exist

## Reply

Simple string `OK`; an existing key is an error.

## Notes

- `R64.SETFULL` materializes containers for the entire u64 range and will exhaust memory — avoid it; use `R64.SETRANGE` over the range you actually need.

## Example

```bash
127.0.0.1:6379> R.SETFULL k
OK
127.0.0.1:6379> R.BITCOUNT k
(integer) 4294967296
```
