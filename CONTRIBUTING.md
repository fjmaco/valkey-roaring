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
- Command replies are a compatibility contract: argument grammar, check
  order, error wording and reply types included (the testing repository's
  suite 09 checks them byte for byte under RESP2 and RESP3). Don't change
  them without updating the command reference (`docs/commands/`, including
  the "Edge cases" and "Limits" sections of `docs/commands/index.md`), the
  tests, and suite 09's expectations.
- The performance table in the README is refreshed by the benchmark
  workflow — don't hand-edit the numbers.
