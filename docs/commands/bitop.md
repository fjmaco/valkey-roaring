# R.BITOP / R64.BITOP

Performs a bitwise set operation over source bitmaps and stores the result in destkey.

| | |
|---|---|
| **Syntax** | `R.BITOP operation destkey srckey srckey [srckey ...]` |
| **64-bit** | `R64.BITOP operation destkey srckey srckey [srckey ...]` |
| **Time complexity** | O(N) |

## Arguments

- **operation** — `AND`, `OR`, `XOR`, `ANDOR`, `DIFF`, `DIFF1`, `ONE` — or `NOT` (see below)
- **destkey** — destination key (overwritten)
- **srckey ...** — at least two sources for the variadic operations; missing keys read as empty

## Reply

Integer: the cardinality of the result.

## Notes

- `ANDOR` = `(src2 ∪ src3 ∪ …) ∩ src1`; `DIFF` = `src1 − src2 − …`; `DIFF1` = `(src2 ∪ …) − src1`; `ONE` = bits present in exactly one source.
- **NOT** takes a single source and an optional universe bound: `R.BITOP NOT dest src [last]` complements over `[0, max(last, src max)]`. A missing/empty source stores an empty bitmap (or the full `[0, last]` range when `last` is given).
- Key positions are reported dynamically through the getkeys API, so cluster slot validation and `COMMAND GETKEYS` handle the non-key `last` argument correctly.

## Example

```bash
127.0.0.1:6379> R.SETINTARRAY a 1 2 3
OK
127.0.0.1:6379> R.SETINTARRAY b 2 3 4
OK
127.0.0.1:6379> R.BITOP XOR dest a b
(integer) 2
127.0.0.1:6379> R.BITOP NOT flipped a 5
(integer) 3
127.0.0.1:6379> R.GETINTARRAY flipped
1) (integer) 0
2) (integer) 4
3) (integer) 5
```
