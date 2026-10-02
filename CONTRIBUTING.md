# Contributing

1. Test this project on your side project or on your live application

OR

1. Open a github issue

OR

1. Fork this project
2. Open a pull request

## Before opening a pull request

CI runs these gates on every PR — running them locally first saves a round trip:

```bash
cargo fmt --check
cargo clippy --release --all-targets -- -D warnings
cargo test --release              # 95 unit and property tests

docker compose up -d              # build + start Valkey with the module
bash tests/integration.sh         # 432 assertions against the live server

# optional, needs a nightly toolchain (CI runs these too):
cargo +nightly fuzz run import_bytes -- -max_total_time=60
```

Notes:

- New commands or behavior changes need coverage in both layers: unit/property
  tests for the algorithm, integration assertions for the wire behavior
  (including wrong-arity and WRONGTYPE cases — the suite checks these
  systematically for every command).
- Command semantics follow [redis-roaring](https://github.com/aviggiano/redis-roaring),
  reply for reply: argument grammar, check order, error wording and reply
  types included. When adding something that exists upstream, match its exact
  bytes (the testing repository's suite 09 compares them under RESP2 and
  RESP3). Diverge only where upstream crashes, hangs, overflows or loses
  data; add the case to the "Differences from redis-roaring" table in
  `docs/commands/index.md` and to suite 09's documented divergences.
- The performance table in the README is refreshed by the benchmark
  workflow — don't hand-edit the numbers.
