# Valkey base image; CI also builds against newer lines (see ci.yml).
ARG VALKEY_VERSION=8.1

FROM rust:1.92-bookworm AS builder

RUN apt-get update && apt-get install -y libclang-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY Cargo.toml Cargo.toml
COPY Cargo.lock Cargo.lock
COPY src/ src/

RUN cargo build --release --locked

FROM valkey/valkey:${VALKEY_VERSION}

COPY --from=builder /build/target/release/libvalkey_roaring.so /usr/lib/valkey/modules/libvalkey_roaring.so

CMD ["valkey-server", "--loadmodule", "/usr/lib/valkey/modules/libvalkey_roaring.so"]
