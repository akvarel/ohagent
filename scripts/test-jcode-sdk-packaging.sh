#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

require_literal() {
  local file="$1"
  local literal="$2"
  if ! grep -Fq -- "$literal" "$file"; then
    printf 'missing required packaging contract in %s: %s\n' "$file" "$literal" >&2
    exit 1
  fi
}

require_literal Dockerfile 'cargo build --manifest-path jcode/Cargo.toml --$BUILD_PROFILE -p jcode --bin jcode --target-dir target/jcode-runtime'
require_literal Dockerfile 'cargo build --manifest-path jcode/Cargo.toml --$BUILD_PROFILE -p jcode-harness-api-server --bin jcode-harness-api-bridge --target-dir target/jcode-runtime'
require_literal Dockerfile 'cp target/jcode-runtime/$BUILD_PROFILE/jcode /out/'
require_literal Dockerfile 'cp target/jcode-runtime/$BUILD_PROFILE/jcode-harness-api-bridge /out/'
require_literal Dockerfile 'COPY --from=builder /out/jcode /usr/local/bin/jcode'
require_literal Dockerfile 'COPY --from=builder /out/jcode-harness-api-bridge /usr/local/bin/jcode-harness-api-bridge'
require_literal Dockerfile 'ENV OHAGENT_JCODE_BINARY=/usr/local/bin/jcode'
require_literal Dockerfile 'ENV OHAGENT_JCODE_RUNTIME_ROOT=/home/jcode/.ohagent/j'

require_literal docker-compose.yml 'OHAGENT_JCODE_BINARY: /usr/local/bin/jcode'
require_literal docker-compose.yml 'OHAGENT_JCODE_RUNTIME_ROOT: /home/jcode/.ohagent/j/compose'

require_literal k8s/base/deployment.yaml 'name: OHAGENT_JCODE_BINARY'
require_literal k8s/base/deployment.yaml 'value: /usr/local/bin/jcode'
require_literal k8s/base/deployment.yaml 'name: OHAGENT_JCODE_RUNTIME_ROOT'
require_literal k8s/base/deployment.yaml 'value: /home/jcode/.ohagent/j/$(POD_UID)'
require_literal k8s/base/deployment.yaml 'fieldPath: metadata.uid'

if command -v docker >/dev/null 2>&1 && docker compose version >/dev/null 2>&1; then
  docker compose config --quiet
fi

printf 'Jcode SDK runtime packaging contract passed.\n'
