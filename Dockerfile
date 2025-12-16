FROM rust:slim AS builder
WORKDIR /app

# Install build dependencies for crates that may need native libs (openssl, etc.)
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates build-essential pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
COPY . .
RUN cargo build --release -p intervals_icu_mcp
RUN strip target/release/server || true

FROM gcr.io/distroless/cc-debian13

WORKDIR /home/nonroot
COPY --from=builder /app/target/release/server /usr/local/bin/intervals_icu_mcp
USER nonroot
ENV RUST_LOG=info
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/intervals_icu_mcp"]
CMD []