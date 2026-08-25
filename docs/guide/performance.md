# Performance

Benchmark methodology follows redis-roaring's performance suite: CRoaring's
`census1881` dataset, full client round-trip latency per command against a
dockerized Valkey, compared with the equivalent native commands. The
benchmark runs in CI whenever performance-relevant code changes and updates
the table below automatically.

Reading the table: point operations (`SETBIT`/`GETBIT`) are round-trip
dominated and land at parity with native commands. The wins appear where
native bitmaps do O(N) work on uncompressed bytes: `BITCOUNT` reads a cached
cardinality, and the `BITOP` family operates on compressed containers —
several times faster on this dataset.

<!--@include: ../../README.md#performance-table-->

Notes: native `MIN`/`MAX` don't exist and `BITOP ANDOR`/`BITOP ONE` are not
supported by Valkey 8.1, so those native rows measure error-reply
round-trips. St.dev. is the per-command standard deviation. Numbers come
from shared GitHub Actions runners — ratios between rows are meaningful,
absolute latencies fluctuate.
