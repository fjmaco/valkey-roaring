# R.SETINTARRAY / R64.SETINTARRAY

Replaces the bitmap with the given integer set.

| | |
|---|---|
| **Syntax** | `R.SETINTARRAY key value [value ...]` |
| **64-bit** | `R64.SETINTARRAY key value [value ...]` |
| **Time complexity** | O(K) for K values |

## Arguments

- **key** — the bitmap key (existing content is discarded)
- **value ...** — the integers to set

## Reply

Simple string `OK`.

## Example

```bash
127.0.0.1:6379> R.SETINTARRAY k 5 1 3 1
OK
127.0.0.1:6379> R.GETINTARRAY k
1) (integer) 1
2) (integer) 3
3) (integer) 5
```
