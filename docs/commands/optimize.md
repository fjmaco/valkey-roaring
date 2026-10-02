# R.OPTIMIZE / R64.OPTIMIZE

Re-chooses container representations for the current data, improving compression, and releases the spare capacity incremental writes leave behind when it is worth a copy (at least an eighth of the value's memory, and 64 bytes per container on average).

| | |
|---|---|
| **Syntax** | `R.OPTIMIZE key` |
| **64-bit** | `R64.OPTIMIZE key` |
| **Time complexity** | O(N) |

## Arguments

- **key** — the bitmap key

## Reply

Simple string `OK`. A missing key is an error (`Roaring: key does not exist`), as in redis-roaring. An optional third argument (upstream's `MEM`) is accepted and changes nothing.

## Notes

- `R.EXPORT` optimizes automatically before serializing (without releasing spare capacity).

## Example

```bash
127.0.0.1:6379> R.OPTIMIZE k
OK
```
