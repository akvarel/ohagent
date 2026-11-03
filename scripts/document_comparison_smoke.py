#!/usr/bin/env python3
"""Independent actual HTTP multi-model acceptance using three LOCAL fixtures.

This is a protocol/configuration integration test, not live OCR or model-quality
measurement. Never sends customer documents or uses actual provider credentials.
"""
import argparse
import base64
from contextlib import ExitStack
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
import urllib.error
import urllib.request

from accounting_document_smoke import port, wait_ready
from comparison_fixture import ComparisonFixture


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True)
    args = parser.parse_args()
    from PIL import Image
    with ExitStack() as cleanup:
        temporary = cleanup.enter_context(tempfile.TemporaryDirectory(prefix="document-comparison-acceptance-"))
        fixture = cleanup.enter_context(ComparisonFixture())
        core_port = port()
        service_key = "artificial-comparison-service-key"
        environment = dict(os.environ, **fixture.environment(temporary), OHAGENT_API_KEY=service_key,
                           OHAGENT_DOCUMENT_BIND=f"127.0.0.1:{core_port}")
        process = None

        def terminate():
            nonlocal process
            if process is not None and process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)

        cleanup.callback(terminate)

        def start():
            nonlocal process
            process = subprocess.Popen([str(Path(args.binary).resolve())], env=environment,
                                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            wait_ready(process, ("127.0.0.1", core_port))

        def request(method, path, data=None, admin=False, expected=200, key=None):
            bearer = key if key is not None else fixture.admin_key if admin else service_key
            headers = {"Content-Type": "application/json", "Authorization": "Bearer " + bearer}
            if not admin:
                headers["X-Tenant-Id"] = "synthetic-comparison-tenant"
            req = urllib.request.Request(f"http://127.0.0.1:{core_port}/api/document-processing/" + path,
                data=json.dumps(data).encode() if data is not None else None, method=method, headers=headers)
            try:
                with urllib.request.urlopen(req, timeout=105) as response:
                    status, body = response.status, response.read()
            except urllib.error.HTTPError as error:
                status, body = error.code, error.read()
            assert status == expected, (method, path, status, expected, body[:500])
            return json.loads(body) if body else None

        image = io.BytesIO()
        Image.new("RGB", (100, 100), "white").save(image, "PNG")
        upload = {"file_name": "artificial-comparison.png", "media_type": "image/png",
                  "content_base64": base64.b64encode(image.getvalue()).decode()}
        start()
        request("GET", "models", admin=True, key=service_key, expected=401)
        settings = request("GET", "models", admin=True)
        assert len(settings["models"]) == 3
        assert "api_key" not in json.dumps(settings) and "base_url" not in json.dumps(settings)
        result = request("POST", "compare", upload)
        comparison = result["comparison"]
        assert comparison["complete"] is True and comparison["disagreements"] == [], result
        assert len(comparison["results"]) == 3 and len(fixture.calls) == 3
        assert {x["status"] for x in comparison["results"]} == {"succeeded"}
        for item in comparison["results"]:
            assert item["model"] == item["extraction"]["model"]
            assert item["model"] != "untrusted-model-label"
        original = json.loads(json.dumps(result))
        fixture.third_disagrees = True
        result = request("POST", "compare", upload)
        assert any("total_amount" in key for key in result["comparison"]["disagreements"]), result
        assert result["comparison"]["results"][2]["extraction"]["fields"]["total_amount"] == "124.45"
        fixture.third_disagrees = False
        fixture.failed_slots = {2}
        result = request("POST", "compare", upload)
        assert result["comparison"]["complete"] is False
        assert result["comparison"]["results"][2]["status"] == "failed"
        assert result["extraction"] is not None
        fixture.failed_slots = {0, 1, 2}
        result = request("POST", "compare", upload)
        assert result["extraction"] is None and result["comparison"]["complete"] is False
        fixture.failed_slots = set()
        settings["models"][2]["enabled"] = False
        settings["models"][0]["model"] = "fixture-model-1-updated-without-rebuild"
        updated = request("PUT", "models", settings, admin=True)
        assert updated["revision"] > settings["revision"]
        request("PUT", "models", settings, admin=True, expected=409)
        before = len(fixture.calls)
        result = request("POST", "compare", upload)
        assert len(fixture.calls) - before == 2
        assert result["comparison"]["results"][2]["status"] == "disabled"
        assert result["comparison"]["results"][0]["model"] == "fixture-model-1-updated-without-rebuild"
        assert result["comparison"]["policy_revision"] == updated["revision"]
        assert original["comparison"]["results"][0]["model"] == "fixture-model-1-v1"
        terminate()
        start()
        assert request("GET", "models", admin=True) == updated, "Registry update lost across restart"
        result = request("POST", "compare", upload)
        assert result["comparison"]["results"][0]["model"] == "fixture-model-1-updated-without-rebuild"
        for model in updated["models"]:
            model["enabled"] = False
        request("PUT", "models", updated, admin=True)
        before = len(fixture.calls)
        request("POST", "compare", upload, expected=503)
        assert len(fixture.calls) == before, "Disabled providers must not receive documents"
        persisted = Path(environment["OHAGENT_DOCUMENT_MODELS_FILE"]).read_text()
        assert "artificial-profile-key" not in persisted and "api_key_env" not in persisted
        print(json.dumps({"status": "PASS", "providers": "3 local HTTP fixtures, not live OCR",
            "checks": ["three independent calls", "strict configured model attribution", "field disagreement",
                       "partial failure", "all failure", "admin-only settings", "disable suppresses calls",
                       "version change without rebuild", "revision conflict", "restart persistence",
                       "historical snapshot preserved", "all disabled fail closed", "no persisted secrets"]}), flush=True)


if __name__ == "__main__":
    main()
