# R.OPTIMIZE / R64.OPTIMIZE

Re-chooses container representations for the current data, improving compression.

| | |
|---|---|
| **Syntax** | `R.OPTIMIZE key` |
| **64-bit** | `R64.OPTIMIZE key` |
| **Time complexity** | O(N) |

## Arguments

- **key** — the bitmap key

## Reply

Simple string `OK` (also for a missing key).

## Notes

- `R.EXPORT` optimizes automatically before serializing.

## Example

```bash
127.0.0.1:6379> R.OPTIMIZE k
OK
```
