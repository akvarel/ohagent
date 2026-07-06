# ── ohAgent Daemon — Multi-stage Docker Build ──
#
# Build:
#   docker build -t ohagent-daemon:latest .
#   docker build -t ohagent-daemon:latest --build-arg JCODE_BRANCH=main .
#
# Run:
#   docker run -p 9090:9090 \
#     -e DEEPSEEK_API_KEY=sk-... \
#     -e TELEGRAM_BOT_TOKEN=123:abc \
#     -v ohagent-data:/home/jcode/.ohagent \
#     ohagent-daemon:latest
#
# Requires jcode submodule to be initialized:
#   git submodule update --init --recursive

# ── Stage 1: Build ──
FROM rust:1.85-bookworm AS builder

ARG JCODE_BRANCH=master
ARG BUILD_PROFILE=release

# Install build dependencies
RUN apt-get update && apt-get install -y \
    protobuf-compiler libprotobuf-dev pkg-config libssl-dev \
    cmake curl git \
    && rm -rf /var/lib/apt/lists/*

# Cache jcode build separately (changes less frequently)
WORKDIR /build/jcode
COPY jcode/ jcode/
COPY ohAgent/Cargo.toml ohAgent/Cargo.lock ohAgent/
COPY ohAgent/crates/ohagent-core/Cargo.toml ohAgent/crates/ohagent-core/
COPY ohAgent/crates/ohagent-daemon/Cargo.toml ohAgent/crates/ohagent-daemon/
COPY ohAgent/crates/ohagent-gateway/Cargo.toml ohAgent/crates/ohagent-gateway/
COPY ohAgent/crates/ohagent-memory/Cargo.toml ohAgent/crates/ohagent-memory/
COPY ohAgent/crates/ohagent-skills/Cargo.toml ohAgent/crates/ohagent-skills/
COPY ohAgent/crates/ohagent-cron/Cargo.toml ohAgent/crates/ohagent-cron/
COPY ohAgent/crates/ohagent-swarm/Cargo.toml ohAgent/crates/ohagent-swarm/

# Copy dummy source files to cache deps
WORKDIR /build/ohAgent
RUN mkdir -p crates/ohagent-core/src crates/ohagent-daemon/src \
    crates/ohagent-gateway/src crates/ohagent-memory/src \
    crates/ohagent-skills/src crates/ohagent-cron/src crates/ohagent-swarm/src \
    && for d in ohagent-core ohagent-daemon ohagent-gateway ohagent-memory ohagent-skills ohagent-cron ohagent-swarm; do \
         echo 'fn main() {}' > crates/$d/src/lib.rs; \
       done \
    && echo 'fn main() {}' > crates/ohagent-daemon/src/main.rs

# Dependency caching build (will fail on jcode, but caches ohAgent deps)
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/ohAgent/target \
    cargo build --$BUILD_PROFILE -p ohagent-daemon || true

# Copy actual source
COPY ohAgent/ .

# Build with jcode
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/ohAgent/target \
    cargo build --$BUILD_PROFILE -p ohagent-daemon \
    && mkdir -p /out \
    && cp target/$BUILD_PROFILE/ohagent-daemon /out/

# ── Stage 2: Runtime ──
FROM debian:bookworm-slim AS runtime

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates curl git openssh-client \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd --create-home --shell /bin/bash jcode

# Copy binary
COPY --from=builder /out/ohagent-daemon /usr/local/bin/ohagent-daemon

# Create data directory
RUN mkdir -p /home/jcode/.ohagent && chown -R jcode:jcode /home/jcode

USER jcode
WORKDIR /home/jcode

# Health check
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
    CMD curl -f http://localhost:9090/health || exit 1

EXPOSE 9090

ENTRYPOINT ["ohagent-daemon"]
CMD ["--health-port", "9090", "--log-level", "info"]
