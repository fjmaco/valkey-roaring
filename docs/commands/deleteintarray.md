# R.DELETEINTARRAY / R64.DELETEINTARRAY

Removes integers from the bitmap.

| | |
|---|---|
| **Syntax** | `R.DELETEINTARRAY key value [value ...]` |
| **64-bit** | `R64.DELETEINTARRAY key value [value ...]` |
| **Time complexity** | O(K) for K values |

## Arguments

- **key** — the bitmap key (created empty if missing)
- **value ...** — the integers to remove

## Reply

Simple string `OK`. Removing absent values (or duplicates) is a safe no-op.

## Example

```bash
127.0.0.1:6379> R.DELETEINTARRAY k 7 7 100
OK
```
