# R.SETFULL / R64.SETFULL

Creates the key with every possible bit set.

| | |
|---|---|
| **Syntax** | `R.SETFULL key` |
| **64-bit** | `R64.SETFULL key` |
| **Time complexity** | O(65,536) containers for 32-bit |

## Arguments

- **key** — must not already exist

## Reply

Simple string `OK`; an existing key is an error.

## Notes

- `R.SETFULL` stores 2³² values as 65,536 run containers (about 3 MB). It is refused when `valkey-roaring.max-write-values` is set below 4294967296, and, with `maxmemory` set, when those 3 MB would push used memory past it (see [limits](/commands/#limits)).
- `R64.SETFULL` is always refused, with `Roaring: range too large: maximum 274877906944 elements` by default: the full 64-bit space would need 2³² full 32-bit sub-bitmaps (2⁴⁸ containers), far more than any server holds. Use `R64.SETRANGE` over the range you actually need.

## Example

```bash
127.0.0.1:6379> R.SETFULL k
OK
127.0.0.1:6379> R.BITCOUNT k
(integer) 4294967296
```
