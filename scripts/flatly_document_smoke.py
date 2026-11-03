#!/usr/bin/env python3
"""Opt-in Flatly HTTP acceptance using a fresh H2 database and synthetic documents.

Starts only loopback services. Uses the existing deterministic acceptance identities,
never production data. Real mail, SMS, push and payment integrations stay disabled.
"""
import argparse
import base64
from datetime import date
import io
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import uuid

from comparison_fixture import ComparisonFixture
from accounting_document_smoke import port, wait_ready


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ohagent-binary", required=True)
    parser.add_argument("--backend-repo", required=True)
    parser.add_argument("--backend-jar", help="Explicit freshly packaged local acceptance JAR")
    parser.add_argument("--admin-key-env", choices=("HOUSEHOLD_DOCUMENTS_ADMIN_KEY", "OHAGENT_DOCUMENT_ADMIN_KEY"), default="HOUSEHOLD_DOCUMENTS_ADMIN_KEY", help="Exercise exactly one documented application admin credential binding")
    parser.add_argument("--provider-base-url", required=True)
    parser.add_argument("--model", required=True)
    parser.add_argument("--key-env", required=True)
    parser.add_argument("--comparison-fixture", action="store_true", help="Use three local HTTP provider fixtures, not live OCR")
    parser.add_argument("--pdf", action="store_true", help="Submit real PDFs through the isolated shared renderer")
    parser.add_argument("--render-image", default="docker.io/library/alpine:3.20",
                        help="Already provisioned local renderer image, never pulled by the service")
    parser.add_argument("--browser-seconds", type=int, default=0, choices=range(0, 901), metavar="0..900")
    args = parser.parse_args()
    key = os.environ.get(args.key_env)
    if not key and not args.comparison_fixture:
        raise SystemExit("Existing provider key is unavailable")
    from PIL import Image, ImageDraw, ImageFont
    root = Path(args.backend_repo).resolve()
    jar = Path(args.backend_jar).resolve() if args.backend_jar else root / "app/target/household-backend-0.1.jar"
    if not jar.is_file():
        raise SystemExit("Freshly packaged backend executable JAR is required")
    processes = []
    fixture = None
    def stop(*_):
        raise SystemExit(0)
    signal.signal(signal.SIGTERM, stop)
    with tempfile.TemporaryDirectory(prefix="flatly-doc-acceptance-") as temporary:
        log_path = Path(temporary) / "startup.log"
        log = log_path.open("w+")
        try:
            core_port, app_port = port(), port()
            service_key = "artificial-flatly-document-service-key"
            core_env = dict(os.environ, OHAGENT_API_KEY=service_key, OHAGENT_DOCUMENT_API_KEY=key,
                            OHAGENT_DOCUMENT_BASE_URL=args.provider_base_url, OHAGENT_DOCUMENT_MODEL=args.model,
                            OHAGENT_DOCUMENT_JSON_MODE="json_object", OHAGENT_DOCUMENT_BIND=f"127.0.0.1:{core_port}")
            if args.comparison_fixture:
                fixture = ComparisonFixture().__enter__()
                fixture.invoice_number = "FLATLY-LIVE-001"
                core_env.update(fixture.environment(temporary, args.pdf, args.render_image))
                core_env.pop("OHAGENT_DOCUMENT_API_KEY", None)
            if args.pdf:
                core_env.update(OHAGENT_DOCUMENT_PDF_MODE="podman", OHAGENT_DOCUMENT_RENDER_IMAGE=args.render_image)
            core = subprocess.Popen([str(Path(args.ohagent_binary).resolve())], env=core_env,
                                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            processes.append(core)
            wait_ready(core, ("127.0.0.1", core_port))
            env = {name: value for name, value in os.environ.items() if name not in (
                args.key_env, "OHAGENT_DOCUMENT_API_KEY", "SENDGRID_API_KEY", "VAULT_TOKEN",
                "GOOGLE_OAUTH_CLIENT_ID", "GOOGLE_OAUTH_CLIENT_SECRET")}
            env.update(JWT_SECRET=uuid.uuid4().hex + uuid.uuid4().hex,
                       DATASOURCES_DEFAULT_URL=f"jdbc:h2:file:{temporary}/test;DB_CLOSE_DELAY=-1;MODE=PostgreSQL;DATABASE_TO_UPPER=false",
                       MICRONAUT_SERVER_PORT=str(app_port), MICRONAUT_SERVER_HOST="127.0.0.1",
                       MICRONAUT_SECURITY_OAUTH2_ENABLED="false", ACCEPTANCE_SEED_ENABLED="true",
                       EMAIL_AUTOMATED_ENABLED="false", SMS_ALLOWED="false", PUSH_ALLOWED="false",
                       HOUSEHOLD_ASSIST_SLM_ENABLED="false", PAYMENT_PROVIDER="manual",
                       APP_PUBLIC_URL=f"http://127.0.0.1:{app_port}",
                       OHAGENT_DOCUMENT_URL=f"http://127.0.0.1:{core_port}/api/document-processing/extract",
                       OHAGENT_API_KEY=service_key, DOCUMENT_EXTRACTION_PROVIDER="ohagent")
            if fixture:
                env.pop("OHAGENT_DOCUMENT_ADMIN_KEY", None)
                env.pop("HOUSEHOLD_DOCUMENTS_ADMIN_KEY", None)
                env[args.admin_key_env] = fixture.admin_key
            app = subprocess.Popen(["java", "-Xmx768m", "-jar", str(jar)], cwd=temporary,
                                   env=env, stdout=log, stderr=log)
            processes.append(app)
            # Startup performs the real Flyway migrations and deterministic test seeding.
            import socket
            for _ in range(600):
                if app.poll() is not None:
                    log.flush()
                    raise RuntimeError("Local Flatly startup failed: " + log_path.read_text()[-6000:])
                try:
                    with socket.create_connection(("127.0.0.1", app_port), timeout=0.1):
                        break
                except OSError:
                    time.sleep(0.1)
            else:
                raise RuntimeError("Local Flatly startup exceeded 60 seconds")
            base = f"http://127.0.0.1:{app_port}"
            bearer = None
            def request(method, path, data=None, expected=200, authenticated=True):
                body = json.dumps(data).encode() if data is not None else None
                headers = {"Content-Type": "application/json"}
                if authenticated and bearer:
                    headers["Authorization"] = "Bearer " + bearer
                req = urllib.request.Request(base + path, data=body, headers=headers, method=method)
                try:
                    with urllib.request.urlopen(req, timeout=120) as response:
                        status, content = response.status, response.read()
                except urllib.error.HTTPError as error:
                    status, content = error.code, error.read()
                if status != expected:
                    log.flush()
                    raise AssertionError(f"{method} {path}: HTTP {status}, expected {expected}: {content[:1000]!r}\n" + log_path.read_text()[-10000:])
                return json.loads(content) if content and content[:1] in (b"{", b"[") else content
            session = request("POST", "/api/auth/login", {"email": "acceptance-manager@test.local", "password": "TestPass123!"}, authenticated=False)
            bearer = session.get("accessToken") or session.get("token") or session.get("access_token")
            assert bearer, "Seeded login did not return an access token"
            manager_bearer = bearer
            outsider = request("POST", "/api/auth/login", {"email": "acceptance-nocompany@test.local", "password": "TestPass123!"}, authenticated=False)
            outsider_bearer = outsider.get("accessToken") or outsider.get("token") or outsider.get("access_token")
            assert outsider_bearer, "Seeded unscoped reviewer did not return an access token"
            request("GET", "/api/document-jobs?companyName=Acceptance%20Company", expected=401, authenticated=False)
            meter = request("POST", "/api/utility-meters", {"objectId": "acceptance-object-1", "unitId": "acceptance-unit-1",
                            "type": "cold_water", "serialNumber": "DOC-LIVE-MTR-001", "status": "active"}, expected=201)
            meter_id = meter["id"]
            reading_date = date.today().isoformat()
            examples = [
                ("invoice", ["ARTIFICIAL FLATLY TEST INVOICE", "Invoice number: FLATLY-LIVE-001", "Issue date: 2026-11-03", "Supplier: Artificial Supplies SIA", "Supplier registration number: 40000000000", "Total amount: 123.45 EUR", "Property address: 1 Test Street, Riga"],
                 {"invoice_number": "FLATLY-LIVE-001", "total_amount": "123.45", "currency": "EUR"}),
                ("meter_reading", ["ARTIFICIAL FLATLY METER REPORT", "Meter serial number: DOC-LIVE-MTR-001", f"Reading date: {reading_date}", "Reading value: 160.00 m3", "Property address: 1 Test Street, Riga", "Apartment number: 1"],
                 {"meter_number": "DOC-LIVE-MTR-001", "reading_value": "160.00", "reading_date": reading_date}),
            ]
            for kind, lines, expected in examples:
                if fixture:
                    fixture.kind = kind
                image = Image.new("RGB", (1400, 800), "white")
                draw = ImageDraw.Draw(image)
                font = ImageFont.truetype("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", 30)
                for index, line in enumerate(lines):
                    draw.text((40, 40 + index * 75), line, font=font, fill="black")
                stream = io.BytesIO()
                image.save(stream, format="PDF" if args.pdf else "PNG")
                extension = "pdf" if args.pdf else "png"
                media_type = "application/pdf" if args.pdf else "image/png"
                upload = {"companyName": "Acceptance Company", "fileName": f"artificial-live-{kind}.{extension}",
                          "mediaType": media_type, "contentBase64": base64.b64encode(stream.getvalue()).decode()}
                job = request("POST", "/api/document-jobs", upload)
                path = "/api/document-jobs/" + job["id"]
                assert job["status"] == "UPLOADED"
                bearer = outsider_bearer
                request("GET", path, expected=403)
                request("GET", path + "/source", expected=403)
                bearer = manager_bearer
                assert request("GET", path + "/source") == stream.getvalue()
                job = request("POST", path + "/analyze")
                assert job["status"] == "REVIEW_REQUIRED", job
                extraction = job["extraction"]
                comparison_snapshot = job.get("comparison")
                if fixture:
                    assert comparison_snapshot and len(comparison_snapshot["results"]) == 3, job
                    assert comparison_snapshot["complete"] and not comparison_snapshot["disagreements"], comparison_snapshot
                    assert all(result["status"] == "succeeded" for result in comparison_snapshot["results"])
                    assert job.get("pilotStartedAt") and job.get("pilotMinimumUntil"), job
                assert extraction["documentType"] == kind, extraction
                for field, value in expected.items():
                    assert extraction["fields"][field] == value, (field, extraction["fields"].get(field))
                approval = {"sourceHash": job["sourceHash"], "documentType": kind, "fields": extraction["fields"]}
                if kind == "invoice":
                    approval.update(propertyId="acceptance-object-1", distributionBasis="PER_UNIT", expenseName="Artificial live document", expenseType="OTHER")
                else:
                    # The acceptance seed already has a different reading today. Preserve the real correction guard.
                    approval.update(meterId="acceptance-meter-1")
                    request("POST", path + "/approve-apply", approval, expected=409)
                    assert request("GET", path)["status"] == "REVIEW_REQUIRED"
                    approval.update(meterId=meter_id)
                applied = request("POST", path + "/approve-apply", approval)
                assert applied["status"] == "APPLIED" and applied["resultId"]
                if fixture:
                    assert applied["comparison"] == comparison_snapshot, "Human review mutated original comparison"
                if kind == "invoice":
                    rows = request("GET", "/api/vendor-invoices?companyName=Acceptance%20Company")
                    domain_rows = [row for row in rows if row["id"] == applied["resultId"]]
                    assert len(domain_rows) == 1 and str(domain_rows[0]["total"]) == "123.45", domain_rows
                    assert domain_rows[0]["documentNumber"] == "FLATLY-LIVE-001", domain_rows
                    assert job["id"] in domain_rows[0]["comment"], domain_rows
                else:
                    rows = request("GET", "/api/utility-meters/readings?meterId=" + meter_id)
                    domain_rows = [row for row in rows["content"] if row["id"] == applied["resultId"]]
                    assert len(domain_rows) == 1 and float(domain_rows[0]["readingValue"]) == 160.0, domain_rows
                    assert domain_rows[0]["readingDate"] == reading_date, domain_rows
                repeated = request("POST", path + "/approve-apply", approval)
                assert applied["resultId"] == repeated["resultId"]
                duplicate = request("POST", "/api/document-jobs", upload)
                assert duplicate["id"] == job["id"]
                print(json.dumps({"status": "PASS", "application": "Flatly", "document_type": kind,
                                  "model": extraction["model"], "provider_kind": "three local HTTP fixtures" if fixture else "live configured providers", "media_type": media_type, "checked_fields": expected,
                                  "source_retained": True, "domain_result_id": applied["resultId"],
                                  "reviewer_retained": bool(applied["reviewedBy"]), "duplicate_effects": False}), flush=True)
            if fixture:
                bearer = manager_bearer
                assert request("GET", "/api/document-models/access")["allowed"] is False
                request("GET", "/api/document-models", expected=403)
                admin_session = request("POST", "/api/auth/login", {"email": "acceptance-admin@test.local", "password": "TestPass123!"}, authenticated=False)
                bearer = admin_session.get("accessToken") or admin_session.get("token") or admin_session.get("access_token")
                assert request("GET", "/api/document-models/access")["allowed"] is True
                settings = request("GET", "/api/document-models")
                assert len(settings["models"]) == 3 and "api_key" not in json.dumps(settings)
                settings["models"][2]["enabled"] = False
                settings["models"][0]["model"] = "fixture-flatly-updated"
                updated = request("PUT", "/api/document-models", settings)
                assert updated["revision"] > settings["revision"]
                request("PUT", "/api/document-models", settings, expected=409)
                bearer = manager_bearer
                assert request("GET", path)["comparison"] == comparison_snapshot
                fixture.kind = "invoice"
                fixture.invoice_number = "FLATLY-UPDATED-001"
                image = Image.new("RGB", (1200, 500), "white")
                ImageDraw.Draw(image).text((30, 30), "ARTIFICIAL UPDATED POLICY INVOICE FLATLY-UPDATED-001 123.45 EUR", fill="black")
                updated_source = io.BytesIO()
                image.save(updated_source, format="PNG")
                updated_upload = {"companyName": "Acceptance Company", "fileName": "artificial-updated-policy.png", "mediaType": "image/png", "contentBase64": base64.b64encode(updated_source.getvalue()).decode()}
                updated_job = request("POST", "/api/document-jobs", updated_upload)
                updated_path = "/api/document-jobs/" + updated_job["id"]
                before_calls = len(fixture.calls)
                updated_job = request("POST", updated_path + "/analyze")
                assert len(fixture.calls) - before_calls == 2
                assert updated_job["comparison"]["complete"] is False
                assert updated_job["comparison"]["results"][2]["status"] == "disabled"
                assert updated_job["comparison"]["results"][0]["model"] == "fixture-flatly-updated"
                assert updated_job["comparison"]["policyRevision"] == updated["revision"]
                unacknowledged = {"sourceHash": updated_job["sourceHash"], "documentType": "invoice", "fields": updated_job["extraction"]["fields"], "propertyId": "acceptance-object-1", "distributionBasis": "PER_UNIT", "expenseName": "Artificial updated policy", "expenseType": "OTHER"}
                request("POST", updated_path + "/approve-apply", unacknowledged, expected=400)
                assert request("GET", updated_path + "/source") == updated_source.getvalue()
                print(json.dumps({"status": "PASS", "application": "Flatly", "checks": ["system-admin settings", "manager forbidden", "dynamic version update", "disabled slot receives no call", "stale revision rejected", "historical model snapshot preserved", "partial comparison requires explicit acknowledgement", "pilot dates persisted"]}), flush=True)
            if args.browser_seconds:
                print(json.dumps({"status": "BROWSER_READY", "backend_url": base,
                                  "lifetime_seconds": args.browser_seconds}), flush=True)
                time.sleep(args.browser_seconds)
        finally:
            for process in reversed(processes):
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)
            if fixture:
                fixture.__exit__(None, None, None)
            log.close()


if __name__ == "__main__":
    main()
