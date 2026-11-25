"""Offline invariants for the owner-approved Authentik registration."""
import importlib.util
import pathlib
import types
import unittest

spec = importlib.util.spec_from_file_location(
    "registration", pathlib.Path(__file__).with_name("register-ohagent-oidc.py")
)
registration = importlib.util.module_from_spec(spec)
spec.loader.exec_module(registration)
namespace = {}
exec("def evaluate(request):\n" + "\n".join("    " + line for line in registration.POLICY.splitlines()), namespace)


class RegistrationTests(unittest.TestCase):
    def allowed(self, **overrides):
        context = dict(oauth_grant_type="authorization_code", oauth_code_challenge_method="S256",
                       oauth_code_challenge="a" * 43, oauth_scopes=["openid", "profile"])
        context.update(overrides)
        return namespace["evaluate"](types.SimpleNamespace(context=context))

    def test_exact_environment_origins_and_callback(self):
        self.assertEqual(registration.TARGETS, (
            ("ohagent-stage", "https://stage.orangehat.ai"),
            ("ohagent-prod", "https://orangehat.ai"),
        ))
        self.assertEqual(registration.CALLBACK, "/agent-as-a-service/app/callback")

    def test_only_s256_authorization_code(self):
        self.assertTrue(self.allowed())
        for grant in ["implicit", "client_credentials", "refresh_token", None]:
            self.assertFalse(self.allowed(oauth_grant_type=grant))
        for method in ["plain", None, ""]:
            self.assertFalse(self.allowed(oauth_code_challenge_method=method))

    def test_challenge_required_and_base64url_bounded(self):
        for challenge in [None, "", "a" * 42, "a" * 44, "+" * 43, "/" * 43, "a" * 42 + "\n"]:
            self.assertFalse(self.allowed(oauth_code_challenge=challenge))
        self.assertTrue(self.allowed(oauth_code_challenge="A_-9" * 10 + "abc"))

    def test_only_required_minimal_scopes(self):
        self.assertTrue(self.allowed(oauth_scopes=["openid"]))
        for scopes in [[], ["profile"], ["openid", "email"], ["openid", "offline_access"], ["openid", "admin"]]:
            self.assertFalse(self.allowed(oauth_scopes=scopes))


if __name__ == "__main__":
    unittest.main()
