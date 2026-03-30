FROM rust:1.82-bookworm AS builder

RUN apt-get update && apt-get install -y libclang-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY Cargo.toml Cargo.toml
COPY src/ src/

RUN cargo build --release

FROM valkey/valkey:8.1

COPY --from=builder /build/target/release/libvalkey_roaring.so /usr/lib/valkey/modules/libvalkey_roaring.so

CMD ["valkey-server", "--loadmodule", "/usr/lib/valkey/modules/libvalkey_roaring.so"]
