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
cargo test --release              # 39 unit and property tests

docker compose up -d              # build + start Valkey with the module
bash tests/integration.sh         # 272 assertions against the live server
```

Notes:

- New commands or behavior changes need coverage in both layers: unit/property
  tests for the algorithm, integration assertions for the wire behavior
  (including wrong-arity and WRONGTYPE cases — the suite checks these
  systematically for every command).
- Command semantics follow [redis-roaring](https://github.com/aviggiano/redis-roaring).
  When adding something that exists upstream, match its behavior; when
  diverging, document why in the PR.
- The performance table in the README is refreshed by the scheduled benchmark
  workflow — don't hand-edit the numbers.
