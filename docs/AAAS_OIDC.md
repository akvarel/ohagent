# Existing Authentik integration for ohAgent

## Approved and applied on November 24, 2026

The owner selected `stage.orangehat.ai` for development/staging and `orangehat.ai` for future production, then explicitly authorized registration of `ohagent-stage` and `ohagent-prod`. Both applications and public OAuth2 providers were registered in the existing Authentik 2026.5.6 service at `auth.bugzero.lv`. This reuses the identity service, not BugZero's confidential clients, sessions, organization roles or billing permissions.

| Environment | Client ID / access-token audience | Exact issuer | Exact authorization callback |
| --- | --- | --- | --- |
| Staging | `ohagent-stage` | `https://auth.bugzero.lv/application/o/ohagent-stage/` | `https://stage.orangehat.ai/agent-as-a-service/app/callback` |
| Future production | `ohagent-prod` | `https://auth.bugzero.lv/application/o/ohagent-prod/` | `https://orangehat.ai/agent-as-a-service/app/callback` |

Non-secret runtime settings are in `config/aaas-stage.env.example` and `config/aaas-prod.env.example`. They intentionally do not enable the customer runtime. Source the relevant file with export enabled into the approved process environment, never into a browser bundle. Registration is not a production application deployment authorization.

## Registration security

- Public clients with an explicitly empty client secret. No secret, password, signing certificate/key or admin API token was generated, copied or exported.
- Only the authorization-code grant is enabled on these providers. No implicit, password, client-credentials or refresh-token grant.
- Strict exact HTTPS callback matching, separately scoped issuers/audiences. No wildcard, localhost, cross-environment or trailing-path callback is registered.
- Dedicated application policy requires authorization code, a 43-character base64url S256 challenge, `openid`, and only `openid profile` scopes. Applications use ALL policy mode. This policy is necessary because the installed Authentik version's public discovery advertises global grants, secret authentication methods and plain PKCE regardless of a provider's restrictions.
- Existing authentication/invalidation flows and existing asymmetric signing-key reference are reused without changing them. Explicit consent flow is used. Access tokens expire after ten minutes, codes after one minute. Existing identities can authenticate subject to the shared authentication flow and the new application's policy. No user, group, membership, enrollment flow or BugZero permission was changed.
- Standard user claims are excluded from ID tokens. The Authentik access-token profile additionally requires `OHAGENT_AAAS_OIDC_REQUIRED_ACCESS_SCOPE=openid` and an `azp` matching the configured client. This distinguishes access tokens from ID tokens even though Authentik uses the client ID as both audiences. Generic providers can leave this optional setting unset.
- Tenant identity remains a hash of validated issuer plus subject, so staging and production remain separate even for the same person. This does not implement immediate membership revocation or global logout.

## Reproducible operations and checks

`scripts/register-ohagent-oidc.py` is limited to the two approved IDs/origins. It defaults to a transaction rolled back after validation, serializes on a stable reference row, validates installed models and five actual Authentik policy-evaluator cases per application, and refuses divergent/partial existing registrations rather than updating them. Setting `OHAGENT_REGISTER_OIDC_APPLY=1` performs registration only after explicit authorization. Do not use the script for unapproved future changes. Run it through the existing Authentik management shell, not a new admin token:

```sh
kubectl --context orangehat-production -n authentik exec -i deployment/authentik-server -c server -- \
  env AUTHENTIK_LOG_LEVEL=error ak shell --no-imports --no-startup \
  -c "exec(compile(__import__('sys').stdin.read(), '<ohagent-registration>', 'exec'), {'__name__': '__main__'})" \
  < scripts/register-ohagent-oidc.py
```

The initial dry run encountered nullable relations that the installed model did not mark `blank`; that transaction rolled back. Validation was corrected for only those optional relations, the complete dry run passed, then the authorized registration committed. A subsequent dry run confirmed `created: false` for both and passed ten installed-evaluator cases. BugZero clients were not updated. No production application workload was deployed or restarted.

Executed checks:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_ohagent_oidc_registration.py
node scripts/check-ohagent-oidc.mjs
cargo test -p ohagent-aaas
cargo clippy -p ohagent-aaas --all-targets --no-deps -- -D warnings
cargo check -p ohagent-daemon
cargo build -p ohagent-daemon
OHAGENT_TEST_DAEMON=/absolute/path/to/ohagent-daemon bash scripts/test-aaas-oidc.sh
```

Results: four offline policy tests passed, ten installed Authentik evaluator cases passed, both real discovery/JWKS/CORS checks passed, both public clients reached invalid-code validation without a secret, and all six foreign/cross-environment/trailing-path redirects were rejected. A same-audience ID-token substitution regression failed before enforcement and passed afterwards. Backend suite: 20 passed, two opt-in model-call tests ignored, scoped strict lint and daemon check/build passed. Both actual local daemon startups discovered their respective live issuer and denied missing/invalid bearer credentials and spoofed tenant headers. Local smoke processes and private fixture databases were cleaned up. No model inference was performed by this follow-up.

`docs/evidence/aaas-001/oidc-registration.json` records the live public checks, explicitly not user-login acceptance. Do not infer client restrictions from global discovery fields alone.

## Remaining deployment and acceptance boundaries

The domain assignments and OIDC registrations are now resolved inputs. The website is not yet deployed to either assigned origin by this follow-up. No DNS records, TLS certificates, registry credentials, provider credentials or production workloads were changed. The current container pipeline does not yet package the dashboard/profile correctly for this new site, and legacy manifests must not be reused unchanged: they include duplicate objects and autoscaling incompatible with the single-owner pilot DB.

Before provisioning a new staging provider credential or changing its storage, obtain the owner's storage choice and relevant authorization. Use a dedicated staging namespace and one SQLite owner, no inherited BugZero runtime credentials or public test-login bypass. Build and deploy through a verified pipeline, enforce staging noindex, then test two real authorized users, callback/PKCE, reconnect/restart and run/artifact isolation on the actual staging origin. The A01–A18 staged acceptance matrix remains open; registration and anonymous/public endpoint checks are not equivalent to live customer authentication.
