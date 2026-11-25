# Staging public preview

Host: https://stage.orangehat.ai. Production `orangehat.ai` is not deployed by this pipeline.

The real `ohagent-daemon --site-only` serves the built dashboard. This is a public preview, **not an enabled customer inference environment**. `OHAGENT_AAAS_ENABLED=0` deliberately closes private operations. The existing labelled demo remains a demo. OIDC registration exists, but live customer login/execution is not claimed. Enabling inference requires a separate approved provider-credential storage decision and full runtime packaging.

## Build and deploy

From the repository root on Linux x86_64 with the repository toolchain, Docker, Node, existing registry authorization and existing cluster/SSH access:

```bash
bash scripts/build-stage-image.sh rg.pl-waw.scw.cloud/orangehat/ohagent:stage-COMMIT-DATE
docker push rg.pl-waw.scw.cloud/orangehat/ohagent:stage-COMMIT-DATE
bash scripts/preload-stage-image.sh rg.pl-waw.scw.cloud/orangehat/ohagent:stage-COMMIT-DATE 62.238.49.165 62.238.48.112 157.180.29.6 2.29.138.63
# Update resources.yaml with the verified imported manifest digest.
kubectl --context orangehat-production apply --dry-run=client -f deploy/stage/resources.yaml
kubectl --context orangehat-production apply -f deploy/stage/resources.yaml
kubectl --context orangehat-production -n ohagent-stage rollout status deployment/ohagent-site --timeout=120s
```

The offline preload is intentional: the registry is private and no new image-pull secret or copied credential is authorized. Preload each eligible node before applying an image update. Inspect `k3s ctr images list` and pin the imported manifest digest, which can differ from the registry manifest. Keep `imagePullPolicy: Never` for this mode. New nodes require the same preload before they can run this workload.

All resources are namespaced `ohagent-stage`, except creation of that namespace. No shared ClusterIssuer, RBAC, or other project workload is changed. The ingress references the existing `letsencrypt-prod` issuer for automatic TLS. One replica and Recreate prevent accidental multi-owner runtime assumptions. There is no persistent customer state in this disabled preview.

DNS: exact A record `stage.orangehat.ai -> 62.238.49.165`, DNS-only, TTL 300. The apex and other records are unchanged. Ingress adds `X-Robots-Tag: noindex, nofollow, noarchive` to staging responses. The image robots file disallows all crawling and does not ship the production sitemap. The image runs non-root with a read-only root filesystem and no service-account token.

## Validation

```bash
curl --fail https://stage.orangehat.ai/health
curl --fail https://stage.orangehat.ai/api/aaas/config
STAGE_SITE_TEST_URL=https://stage.orangehat.ai node --test crates/ohagent-dashboard/tests/stage-http.mjs
PUBLIC_UI_BASE_URL=https://stage.orangehat.ai npm --prefix crates/ohagent-dashboard run test:public
```

If a local router still caches the previous NXDOMAIN, first verify the new A record against the authoritative nameserver and a public resolver. For isolated acceptance only, use `STAGE_SITE_TEST_ADDRESS=62.238.49.165` and `PLAYWRIGHT_HOST_RESOLVER_RULES='MAP stage.orangehat.ai 62.238.49.165'`. These override routing only and do not disable HTTPS certificate validation or modify system DNS.

Expected config: `pilot_available=false`, `auth=null`. Private endpoints must not become successful SPA documents. Verify trusted TLS, nested routes, missing assets/API status, robots header and anonymous denial before reporting the site available. Do not use the production-discovery assertion in `site-http.mjs` unchanged against staging, which intentionally does not advertise the production sitemap.

To roll back, preload a previously verified image and change only this deployment's digest, then apply the same manifest. Do not roll back DNS to the production apex or enable inference as a workaround.
