FROM rust:1.96-slim AS builder
WORKDIR /app

# btleplug links against system D-Bus
RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libdbus-1-dev \
    && rm -rf /var/lib/apt/lists/*

# Build dependencies with empty targets
COPY ./Cargo.toml ./Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && echo "" > src/lib.rs
RUN cargo build --release

# Copy in src, touch files to set modified time, then build
COPY ./src src
RUN touch src/main.rs src/lib.rs
RUN cargo build --release

# Copy binary to release image
FROM debian:13.5-slim
WORKDIR /app
RUN apt-get update \
    && apt-get install -y --no-install-recommends libdbus-1-3 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/dewpoint .
EXPOSE 9185
CMD ["./dewpoint"]
