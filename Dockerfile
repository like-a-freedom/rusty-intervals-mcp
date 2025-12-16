FROM rust:slim AS builder
WORKDIR /app

# Install build dependencies for crates that may need native libs (openssl, etc.)
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates build-essential pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
COPY . .
# Build all binaries for the package (including the HTTP `server` binary).
RUN cargo build --release -p intervals_icu_mcp --locked
# Strip the HTTP server binary for smaller image size (best-effort)
RUN strip target/release/server || true

FROM gcr.io/distroless/cc-debian13

WORKDIR /home/nonroot
# Use the compiled HTTP server binary in the final image.
COPY --from=builder /app/target/release/server /usr/local/bin/intervals_icu_mcp
USER nonroot
ENV RUST_LOG=info
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/intervals_icu_mcp"]
CMD []