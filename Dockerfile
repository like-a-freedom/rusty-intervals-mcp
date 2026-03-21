FROM rust:slim AS builder
WORKDIR /app

# Install build dependencies for crates that may need native libs (openssl, etc.)
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates build-essential pkg-config libssl-dev cmake ninja-build perl python3 git clang libclang-dev \
    && rm -rf /var/lib/apt/lists/*
COPY . .
RUN RUSTC_WRAPPER= cargo build --release -p intervals_icu_mcp --bin intervals_icu_mcp
RUN strip target/release/intervals_icu_mcp || true

FROM gcr.io/distroless/cc-debian13

WORKDIR /home/nonroot
COPY --from=builder /app/target/release/intervals_icu_mcp /usr/local/bin/intervals_icu_mcp
USER nonroot
ENV MCP_TRANSPORT=http
ENV MCP_HTTP_ADDRESS=0.0.0.0:3000
ENV MAX_HTTP_BODY_SIZE=4194304
ENV RUST_LOG=info
EXPOSE 3000
ENTRYPOINT ["/usr/local/bin/intervals_icu_mcp"]
CMD []