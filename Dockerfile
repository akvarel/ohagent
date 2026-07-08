# ── ohAgent Daemon — Multi-stage Docker Build ──
FROM rust:latest AS builder

ARG BUILD_PROFILE=release

RUN apt-get update && apt-get install -y \
    protobuf-compiler libprotobuf-dev pkg-config libssl-dev \
    cmake curl git \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy workspace manifests
COPY Cargo.toml Cargo.lock ./
COPY crates/ohagent-core/Cargo.toml crates/ohagent-core/
COPY crates/ohagent-daemon/Cargo.toml crates/ohagent-daemon/
COPY crates/ohagent-gateway/Cargo.toml crates/ohagent-gateway/
COPY crates/ohagent-memory/Cargo.toml crates/ohagent-memory/
COPY crates/ohagent-skills/Cargo.toml crates/ohagent-skills/
COPY crates/ohagent-cron/Cargo.toml crates/ohagent-cron/
COPY crates/ohagent-swarm/Cargo.toml crates/ohagent-swarm/
COPY crates/ohagent-reasoning/Cargo.toml crates/ohagent-reasoning/
COPY crates/ohagent-desktop-mcp/Cargo.toml crates/ohagent-desktop-mcp/
COPY crates/ohagent-plugins/Cargo.toml crates/ohagent-plugins/
COPY crates/ohagent-provider-metrics/Cargo.toml crates/ohagent-provider-metrics/

# Stub Cargo.toml for excluded crates
RUN for crate in ohagent-pii-redactor ohagent-infra-launcher ohagent-aggregator-core ohagent-aggregator-plugin; do \
      mkdir -p crates/$crate/src; \
      printf '[package]\nname = "%s"\nversion.workspace = true\nedition.workspace = true\n' "$crate" > crates/$crate/Cargo.toml; \
      echo '' > crates/$crate/src/lib.rs; \
    done

# Copy jcode submodule (needed first for dep resolution)
COPY jcode/ jcode/

# Create dummy source for dep caching
RUN mkdir -p crates/ohagent-core/src crates/ohagent-daemon/src \
    crates/ohagent-gateway/src crates/ohagent-memory/src \
    crates/ohagent-skills/src crates/ohagent-cron/src crates/ohagent-swarm/src \
    crates/ohagent-reasoning/src crates/ohagent-desktop-mcp/src \
    crates/ohagent-plugins/src crates/ohagent-provider-metrics/src \
    && for d in ohagent-core ohagent-daemon ohagent-gateway ohagent-memory \
              ohagent-skills ohagent-cron ohagent-swarm ohagent-reasoning \
              ohagent-desktop-mcp ohagent-plugins ohagent-provider-metrics; do \
         echo '' > crates/$d/src/lib.rs; \
       done \
    && echo 'fn main() {}' > crates/ohagent-daemon/src/main.rs

# Pre-build for dependency caching
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --$BUILD_PROFILE -p ohagent-daemon || true

# Copy actual ohAgent source
COPY crates/ crates/

# Touch all source files to bust cargo's incremental cache
RUN find crates -name "*.rs" -exec touch {} +

# Final build
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --$BUILD_PROFILE -p ohagent-daemon \
    && mkdir -p /out \
    && cp target/$BUILD_PROFILE/ohagent-daemon /out/

# ── Stage 2: Runtime ──
FROM debian:trixie-slim AS runtime

RUN apt-get update && apt-get install -y \
    ca-certificates curl git openssh-client \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --create-home --shell /bin/bash jcode
COPY --from=builder /out/ohagent-daemon /usr/local/bin/ohagent-daemon
RUN mkdir -p /home/jcode/.ohagent && chown -R jcode:jcode /home/jcode

USER jcode
WORKDIR /home/jcode

HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
    CMD curl -f http://localhost:9090/health || exit 1

EXPOSE 9090

ENTRYPOINT ["ohagent-daemon"]
CMD ["--health-port", "9090", "--log-level", "info"]
