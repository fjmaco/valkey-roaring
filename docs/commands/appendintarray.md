# R.APPENDINTARRAY / R64.APPENDINTARRAY

Adds integers to the bitmap.

| | |
|---|---|
| **Syntax** | `R.APPENDINTARRAY key value [value ...]` |
| **64-bit** | `R64.APPENDINTARRAY key value [value ...]` |
| **Time complexity** | O(K) for K values |

## Arguments

- **key** — the bitmap key (created if missing)
- **value ...** — the integers to add

## Reply

Simple string `OK`.

## Example

```bash
127.0.0.1:6379> R.APPENDINTARRAY k 7 9
OK
```
