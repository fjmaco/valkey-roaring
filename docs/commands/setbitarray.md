# R.SETBITARRAY / R64.SETBITARRAY

Replaces the bitmap from an ASCII bit string: character at index i sets bit i when it is `1`.

| | |
|---|---|
| **Syntax** | `R.SETBITARRAY key bitstring` |
| **64-bit** | `R64.SETBITARRAY key bitstring` |
| **Time complexity** | O(L) for string length L |

## Arguments

- **key** — the bitmap key (existing content is discarded)
- **bitstring** — a string of `0`/`1` characters; any other character counts as `0`

## Reply

Simple string `OK`.

## Example

```bash
127.0.0.1:6379> R.SETBITARRAY k 0101
OK
127.0.0.1:6379> R.GETINTARRAY k
1) (integer) 1
2) (integer) 3
```
