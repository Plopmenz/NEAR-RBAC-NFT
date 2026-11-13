# Build from the repository root so the workspace and Cargo.lock are available:
#
#   docker build -f services/oidc/Dockerfile -t payrole-oidc .
#   docker run --rm -p 8080:8080 \
#     -v "$PWD/services/oidc/config.toml:/app/config.toml:ro" \
#     -v "$PWD/services/oidc/signing-key.pem:/app/signing-key.pem:ro" \
#     payrole-oidc

FROM rust:1.95-slim-bookworm AS builder
WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends build-essential pkg-config \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY contracts ./contracts
COPY services ./services

RUN cargo build -p payrole-oidc --release

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home payrole

WORKDIR /app
COPY --from=builder /app/target/release/payrole-oidc /usr/local/bin/payrole-oidc

USER payrole
EXPOSE 8080

ENTRYPOINT ["payrole-oidc"]
CMD ["/app/config.toml"]
