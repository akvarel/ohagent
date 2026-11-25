"""Register the two owner-approved public ohAgent clients in existing Authentik.

Run through `ak shell --no-imports --no-startup`, feeding this file on stdin.
Default is a rolled-back dry run. Set OHAGENT_REGISTER_OIDC_APPLY=1 only for
an explicitly approved registration. Existing divergent objects cause failure,
never updates. No passwords, client secrets, tokens or signing keys are created.
"""
import json
import os

TARGETS = (
    ("ohagent-stage", "https://stage.orangehat.ai"),
    ("ohagent-prod", "https://orangehat.ai"),
)
CALLBACK = "/agent-as-a-service/app/callback"
POLICY = """import re
return (
    request.context.get('oauth_grant_type') == 'authorization_code'
    and request.context.get('oauth_code_challenge_method') == 'S256'
    and re.fullmatch(r'[A-Za-z0-9_-]{43}', request.context.get('oauth_code_challenge') or '') is not None
    and 'openid' in request.context.get('oauth_scopes', [])
    and set(request.context.get('oauth_scopes', [])) <= {'openid', 'profile'}
)
"""


def register():
    from django.db import transaction
    from django.contrib.auth.models import AnonymousUser
    from authentik.policies.types import PolicyRequest
    from authentik.core.models import Application
    from authentik.flows.models import Flow
    from authentik.policies.expression.models import ExpressionPolicy
    from authentik.policies.models import PolicyBinding
    from authentik.providers.oauth2.models import OAuth2Provider, ScopeMapping

    apply = os.environ.get("OHAGENT_REGISTER_OIDC_APPLY") == "1"
    with transaction.atomic():
        reference = OAuth2Provider.objects.get(client_id="bugzero-production")
        assert reference.signing_key_id, "Existing asymmetric signing key required"
        auth_flow = Flow.objects.get(slug="default-authentication-flow")
        consent = Flow.objects.get(slug="default-provider-authorization-explicit-consent")
        invalidation = Flow.objects.get(slug="default-provider-invalidation-flow")
        mappings = list(ScopeMapping.objects.filter(managed__in=[
            "goauthentik.io/providers/oauth2/scope-openid",
            "goauthentik.io/providers/oauth2/scope-profile",
        ]))
        assert {m.scope_name for m in mappings} == {"openid", "profile"}, "Default scope mappings required"
        # Lock the stable reference row to serialize concurrent registrations.
        OAuth2Provider.objects.select_for_update().get(pk=reference.pk)
        for client_id, origin in TARGETS:
            redirect = [{"matching_mode": "strict", "url": origin + CALLBACK, "redirect_uri_type": "authorization"}]
            expected = dict(
                name=client_id, client_type="public", client_id=client_id,
                client_secret="", grant_types=["authorization_code"],
                _redirect_uris=redirect, issuer_mode="per_provider",
                include_claims_in_id_token=False, sub_mode="hashed_user_id",
                access_code_validity="minutes=1", access_token_validity="minutes=10",
                authentication_flow_id=auth_flow.pk, authorization_flow_id=consent.pk,
                invalidation_flow_id=invalidation.pk, signing_key_id=reference.signing_key_id,
            )
            provider = OAuth2Provider.objects.filter(client_id=client_id).first()
            created = provider is None
            if created:
                assert not OAuth2Provider.objects.filter(name=client_id).exists(), "Provider name collision"
                provider = OAuth2Provider(**expected)
                # Authentik's optional nullable relations are not marked blank
                # in the Django model, although its API permits null values.
                provider.full_clean(exclude=["backchannel_application", "encryption_key"])
                provider.save()
                provider.property_mappings.set(mappings)
            else:
                assert all(getattr(provider, k) == v for k, v in expected.items()), "Existing provider differs; explicit review required"
                assert set(provider.property_mappings.values_list("pk", flat=True)) == {m.pk for m in mappings}, "Existing scopes differ"
            policy, policy_created = ExpressionPolicy.objects.get_or_create(
                name=client_id + "-require-s256", defaults={"expression": POLICY}
            )
            assert policy.expression == POLICY, "Existing policy differs"
            baseline = dict(oauth_grant_type="authorization_code", oauth_code_challenge_method="S256",
                            oauth_code_challenge="a" * 43, oauth_scopes=["openid", "profile"])
            for override, allowed in [({}, True), ({"oauth_code_challenge_method": "plain"}, False),
                                      ({"oauth_code_challenge": None}, False),
                                      ({"oauth_scopes": ["openid", "offline_access"]}, False),
                                      ({"oauth_grant_type": "client_credentials"}, False)]:
                request = PolicyRequest(AnonymousUser())
                request.context = {**baseline, **override}
                assert policy.passes(request).passing == allowed, "Installed policy evaluation failed"
            app, app_created = Application.objects.get_or_create(slug=client_id, defaults={
                "name": client_id, "provider": provider, "meta_launch_url": origin + "/agent-as-a-service/app",
                "policy_engine_mode": "all",
            })
            assert app.provider_id == provider.pk and app.policy_engine_mode == "all", "Existing application differs"
            assert app.meta_launch_url == origin + "/agent-as-a-service/app", "Existing launch URL differs"
            binding, binding_created = PolicyBinding.objects.get_or_create(target=app, order=0, defaults={"policy": policy, "enabled": True})
            assert binding.policy_id == policy.pk and binding.enabled and not binding.negate, "Existing binding differs"
            assert PolicyBinding.objects.filter(target=app).count() == 1, "Unexpected extra policy binding"
            assert created == app_created == policy_created == binding_created, "Partial registration detected"
            print(json.dumps({"client_id": client_id, "origin": origin, "callback": origin + CALLBACK,
                "issuer": "https://auth.bugzero.lv/application/o/" + client_id + "/",
                "client_type": "public", "secret": "none", "s256_policy": True, "installed_policy_checks": 5,
                "created": created, "applied": apply}))
        if not apply:
            transaction.set_rollback(True)
            print("OHAGENT_OIDC_DRY_RUN_ROLLED_BACK")
        else:
            print("OHAGENT_OIDC_REGISTRATION_COMMITTED")


if __name__ == "__main__":
    register()
