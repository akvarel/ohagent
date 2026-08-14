# ── ohAgent Daemon — Multi-stage Docker Build ──
# Architecture:
#   Stage 1 (builder):   full Rust toolchain, compiles ohagent-daemon
#   Stage 2 (runtime):   minimal Debian image, runs the binary
#
# Excluded proprietary crates (stubbed as no-ops):
#   ohagent-pii-redactor, ohagent-infra-launcher,
#   ohagent-aggregator-core, ohagent-aggregator-plugin
#
# These crates have `Proprietary` licenses and are replaced with
# auto-generated empty Cargo.toml + lib.rs during the dependency cache
# phase to allow Cargo workspace resolution. The real source is copied
# later but only crates listed below are actually compiled.
#
# To include a proprietary crate: add its Cargo.toml to the COPY list
# and remove it from the stub loop below.
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

# Stub proprietary crates (no-ops for Cargo workspace resolution)
RUN for crate in ohagent-pii-redactor ohagent-infra-launcher \
                 ohagent-aggregator-core ohagent-aggregator-plugin; do \
      mkdir -p crates/$crate/src; \
      printf '[package]\nname = "%s"\nversion.workspace = true\nedition.workspace = true\n' "$crate" > crates/$crate/Cargo.toml; \
      echo '// stub — replaced by proprietary version in prod builds' > crates/$crate/src/lib.rs; \
    done

# Copy jcode submodule (required for path deps)
COPY jcode/ jcode/

# Create dummy source for dependency caching
RUN mkdir -p crates/ohagent-core/src crates/ohagent-daemon/src \
    crates/ohagent-gateway/src crates/ohagent-memory/src \
    crates/ohagent-skills/src crates/ohagent-cron/src crates/ohagent-swarm/src \
    crates/ohagent-reasoning/src crates/ohagent-desktop-mcp/src \
    crates/ohagent-plugins/src crates/ohagent-provider-metrics/src \
    && for d in ohagent-core ohagent-daemon ohagent-gateway ohagent-memory \
              ohagent-skills ohagent-cron ohagent-swarm ohagent-reasoning \
              ohagent-desktop-mcp ohagent-plugins ohagent-provider-metrics; do \
         echo '// dummy — replaced by real source below' > crates/$d/src/lib.rs; \
       done \
    && echo 'fn main() {}' > crates/ohagent-daemon/src/main.rs

# Pre-build for dependency caching (intentionally may fail)
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --$BUILD_PROFILE -p ohagent-daemon || echo "[cache] Dep caching done (expected: stub failures ignored)"

# Copy actual ohAgent source (overwrites stubs)
COPY crates/ crates/

# Touch all source files to bust cargo's incremental cache
RUN find crates -name "*.rs" -exec touch {} +

# Final build. ohAgent uses Jcode through the public SDK, which launches the
# Jcode runtime as a separate process. Ship both binaries from the same source
# revision so the SDK and runtime protocol stay compatible.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --$BUILD_PROFILE -p ohagent-daemon \
    && cargo build --manifest-path jcode/Cargo.toml --$BUILD_PROFILE -p jcode --bin jcode --target-dir target/jcode-runtime \
    && mkdir -p /out \
    && cp target/$BUILD_PROFILE/ohagent-daemon /out/ \
    && cp target/jcode-runtime/$BUILD_PROFILE/jcode /out/

# ── Stage 2: Runtime ──
FROM debian:trixie-slim AS runtime

RUN apt-get update && apt-get install -y \
    ca-certificates curl git openssh-client \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --create-home --shell /bin/bash jcode
COPY --from=builder /out/ohagent-daemon /usr/local/bin/ohagent-daemon
COPY --from=builder /out/jcode /usr/local/bin/jcode
ENV OHAGENT_JCODE_BINARY=/usr/local/bin/jcode
ENV OHAGENT_JCODE_RUNTIME_ROOT=/home/jcode/jr
RUN mkdir -p /home/jcode/jr && chown -R jcode:jcode /home/jcode

USER jcode
WORKDIR /home/jcode

HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
    CMD curl -f http://localhost:9090/health || exit 1

EXPOSE 9090

ENTRYPOINT ["ohagent-daemon"]
CMD ["--health-port", "9090", "--log-level", "info"]
