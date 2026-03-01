# Stage 1: Build the frontend
FROM node:22-slim AS frontend-builder
WORKDIR /app/frontend

# Install pnpm
RUN corepack enable && corepack prepare pnpm@latest --activate

# Copy frontend source
# We copy the entire frontend folder but we need to ensure pnpm-lock.yaml is there
COPY frontend/package.json frontend/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile

COPY frontend/ ./
RUN pnpm build

# Stage 2: Build the Rust resolver
FROM rust:1.93-slim-trixie AS resolver-builder
WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libnftables-dev \
    libsqlite3-dev \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace configuration
COPY Cargo.toml Cargo.lock ./

# Copy frontend crate (needed because resolver depends on it)
COPY frontend/Cargo.toml frontend/Cargo.toml
COPY frontend/src/lib.rs frontend/src/lib.rs
COPY frontend/build.rs frontend/build.rs
# Copy the built frontend dist from previous stage
COPY --from=frontend-builder /app/frontend/dist frontend/dist

# Copy resolver crate
COPY resolver/ resolver/

# Build the resolver in release mode
RUN cargo build --release --bin resolver

# Stage 3: Final runner image
FROM debian:trixie-slim
WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    nftables \
    ca-certificates \
    sqlite3 \
    && rm -rf /var/lib/apt/lists/*

# Copy the binary from the builder stage
COPY --from=resolver-builder /app/target/release/resolver /usr/local/bin/monadns-resolver

# Create directory for data (if needed, based on config)
RUN mkdir -p /opt/monadns

# Exposure of ports (DNS: 53 UDP/TCP, API/Web: 8080 or as configured)
EXPOSE 53/udp 53/tcp 8080/tcp

ENTRYPOINT ["monadns-resolver"]
