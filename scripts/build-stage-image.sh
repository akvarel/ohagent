#!/usr/bin/env bash
# Build the real site-only daemon from the locked workspace, without provider secrets.
set -euo pipefail
cd "$(dirname "$0")/.."
IMAGE="${1:?usage: build-stage-image.sh registry/image:immutable-tag}"
CONTEXT=$(mktemp -d)
CONTAINER=""
cleanup() {
  if [[ -n "$CONTAINER" ]]; then docker rm -f "$CONTAINER" >/dev/null; fi
  rm -rf "$CONTEXT"
}
trap cleanup EXIT
cargo build --locked -p ohagent-daemon
npm --prefix crates/ohagent-dashboard ci
npm --prefix crates/ohagent-dashboard run build
TARGET=$(cargo metadata --no-deps --format-version 1 | node -e 'let s="";process.stdin.on("data",x=>s+=x).on("end",()=>console.log(JSON.parse(s).target_directory))')
install -m 755 "$TARGET/debug/ohagent-daemon" "$CONTEXT/ohagent-daemon"
strip "$CONTEXT/ohagent-daemon"
cp -R target/dashboard-dist "$CONTEXT/site"
printf 'User-agent: *\nDisallow: /\n' > "$CONTEXT/site/robots.txt"
# Staging must not advertise the production sitemap.
rm -f "$CONTEXT/site/sitemap.xml"
cp deploy/stage/Dockerfile "$CONTEXT/Dockerfile"
docker build --label "org.opencontainers.image.revision=$(git rev-parse HEAD)" -t "$IMAGE" "$CONTEXT"
CONTAINER=$(docker run --detach --read-only --tmpfs /tmp:rw,size=64m -p 127.0.0.1::9090 "$IMAGE")
PORT=$(docker port "$CONTAINER" 9090/tcp | cut -d: -f2)
curl --fail --silent --show-error --retry 10 --retry-connrefused --retry-delay 1 "http://127.0.0.1:$PORT/health"
curl --fail --silent --show-error "http://127.0.0.1:$PORT/api/aaas/config" | node -e 'let s="";process.stdin.on("data",x=>s+=x).on("end",()=>{const c=JSON.parse(s);if(c.pilot_available!==false||c.auth!==null)process.exit(1)})'
