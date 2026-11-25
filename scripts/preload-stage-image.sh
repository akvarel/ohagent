#!/usr/bin/env bash
# Offline distribution avoids copying registry credentials into the staging namespace.
# Each node must already allow the operator's SSH identity. No credentials are created.
set -euo pipefail
IMAGE="${1:?usage: preload-stage-image.sh image node-ip [node-ip ...]}"
shift
[[ $# -gt 0 ]]
for node in "$@"; do
  docker image save "$IMAGE" | ssh -o BatchMode=yes -o ConnectTimeout=10 "root@$node" 'k3s ctr images import -'
  ssh -o BatchMode=yes -o ConnectTimeout=10 "root@$node" bash -s -- "$IMAGE" <<'REMOTE'
set -euo pipefail
image="$1"
digest=$(k3s ctr images list | awk -v image="$image" '$1 == image {print $3}')
[[ "$digest" == sha256:* ]]
repository="${image%:*}"
k3s ctr images tag "$image" "$repository@$digest"
REMOTE
done
