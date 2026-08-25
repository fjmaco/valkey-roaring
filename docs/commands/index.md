# Command Reference

Every command exists in a 32-bit (`R.*`, values 0 … 2³²−1) and a 64-bit
(`R64.*`, values 0 … 2⁶⁴−1) form with identical semantics — both are
generated from one generic implementation. Each page below documents the
family once. `R.STAT` is shared and auto-detects the key's width.

**Total: 51 commands** (25 `R.*` + 25 `R64.*` + `R.STAT`).

| Group | Commands |
|-------|----------|
| Bit access | [SETBIT](/commands/setbit), [GETBIT](/commands/getbit), [GETBITS](/commands/getbits), [CLEARBITS](/commands/clearbits), [CLEAR](/commands/clear) |
| Integer arrays | [SETINTARRAY](/commands/setintarray), [GETINTARRAY](/commands/getintarray), [APPENDINTARRAY](/commands/appendintarray), [DELETEINTARRAY](/commands/deleteintarray), [RANGEINTARRAY](/commands/rangeintarray) |
| Bit strings | [SETBITARRAY](/commands/setbitarray), [GETBITARRAY](/commands/getbitarray) |
| Ranges | [SETRANGE](/commands/setrange), [SETFULL](/commands/setfull) |
| Aggregation | [BITCOUNT](/commands/bitcount), [BITPOS](/commands/bitpos), [MIN](/commands/min), [MAX](/commands/max) |
| Set algebra | [BITOP](/commands/bitop), [CONTAINS](/commands/contains), [JACCARD](/commands/jaccard), [DIFF](/commands/diff) |
| Interchange | [EXPORT](/commands/export), [IMPORT](/commands/import) |
| Maintenance | [OPTIMIZE](/commands/optimize), [STAT](/commands/stat) |

Replies for 64-bit values above 2⁶³−1 arrive as decimal strings (RESP
integers are signed 64-bit); everything else is a plain integer reply.
