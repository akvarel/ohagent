#!/usr/bin/env python3
"""Opt-in real accounting + live vision acceptance, using disposable synthetic data only.

Requires the labelled localhost test PostgreSQL container, built ohAgent and
accounting gateway binaries, Pillow, and an already available provider key.
Never targets a configured production URL or submits customer documents.
"""
import argparse
import base64
import hashlib
import hmac
import io
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import threading
import json
import os
from pathlib import Path
import socket
import signal
import subprocess
import time
import urllib.error
import urllib.request
import uuid


def port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def wait_ready(process, address):
    for _ in range(200):
        if process.poll() is not None:
            raise RuntimeError("Synthetic acceptance service exited before readiness")
        try:
            with socket.create_connection(address, timeout=0.1):
                return
        except OSError:
            time.sleep(0.05)
    raise RuntimeError("Synthetic acceptance readiness timeout")


def token(secret, tenant):
    def encode(value):
        return base64.urlsafe_b64encode(json.dumps(value, separators=(",", ":")).encode()).rstrip(b"=")
    message = encode({"alg": "HS256", "typ": "JWT"}) + b"." + encode({
        "sub": "artificial-acceptance-reviewer", "email": "fixture@example.invalid",
        "tenant_id": tenant, "role": "member", "iat": int(time.time()), "exp": int(time.time()) + 600})
    signature = base64.urlsafe_b64encode(hmac.new(secret.encode(), message, hashlib.sha256).digest()).rstrip(b"=")
    return (message + b"." + signature).decode()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ohagent-binary", required=True)
    parser.add_argument("--gateway-binary", required=True)
    parser.add_argument("--accounting-repo", required=True)
    parser.add_argument("--provider-base-url", required=True)
    parser.add_argument("--model", required=True)
    parser.add_argument("--key-env", required=True)
    parser.add_argument("--container", default="ohagent-documents-acceptance-20261103")
    parser.add_argument("--db-port", type=int, default=25439)
    parser.add_argument("--browser-seconds", type=int, default=0,
                        help="Keep this synthetic backend alive for browser validation, maximum 900 seconds")
    args = parser.parse_args()
    def stop_fixture(*_):
        raise SystemExit(0)
    signal.signal(signal.SIGTERM, stop_fixture)
    if not 0 <= args.browser_seconds <= 900:
        raise SystemExit("Browser fixture lifetime must be between 0 and 900 seconds")
    provider_key = os.environ.get(args.key_env)
    if not provider_key:
        raise SystemExit("Existing provider key is unavailable")
    label = subprocess.check_output(["podman", "inspect", "--format", '{{index .Config.Labels "purpose"}}', args.container], text=True).strip()
    if label != "ohagent-document-acceptance":
        raise SystemExit("Refusing database without disposable acceptance label")
    published = subprocess.check_output(["podman", "port", args.container, "5432/tcp"], text=True).strip()
    if published != f"127.0.0.1:{args.db_port}":
        raise SystemExit("Refusing database port not owned by the labelled localhost container")
    from PIL import Image, ImageDraw, ImageFont

    schema = "document_acceptance_" + uuid.uuid4().hex
    processes = []

    def sql(statement):
        result = subprocess.run(["podman", "exec", "-i", args.container, "psql", "-X", "-At", "-v", "ON_ERROR_STOP=1", "-U", "document_test", "-d", "document_test"], input=statement, text=True, capture_output=True, timeout=15)
        if result.returncode:
            raise RuntimeError("Synthetic database setup/assertion failed: " + result.stderr[-2000:])
        return result.stdout.strip()

    def scoped(statement):
        return sql(f"SET search_path TO {schema};\n" + statement)

    try:
        sql(f"CREATE SCHEMA {schema};")
        migrations = Path(args.accounting_repo) / "shared/db/migrations"
        for name in ("001_tenants.sql", "002_accounting_core.sql", "004_organization.sql", "010_document_jobs.sql"):
            scoped((migrations / name).read_text())
        tenant_a, tenant_b, supplier_a, supplier_b = [str(uuid.uuid4()) for _ in range(4)]
        scoped(f"""
INSERT INTO tenants(id,name,slug,tax_regime) VALUES
 ('{tenant_a}','Artificial Acceptance','artificial-a','SIA'),('{tenant_b}','Artificial Foreign','artificial-b','SIA');
INSERT INTO accounts(code,tenant_id,name,account_type) VALUES
 ('6300','{tenant_a}','Artificial expense','Expense'),('2110','{tenant_a}','Artificial payable','Liability');
INSERT INTO suppliers(id,tenant_id,name,registration_number) VALUES
 ('{supplier_a}','{tenant_a}','Artificial Supplies SIA','40000000000'),
 ('{supplier_b}','{tenant_b}','Artificial Foreign Supplier','40000000000');
""")
        core_port, gateway_port = port(), port()
        service_key = "artificial-local-acceptance-service-key"
        jwt_secret = uuid.uuid4().hex + uuid.uuid4().hex
        env = dict(os.environ, OHAGENT_API_KEY=service_key, OHAGENT_DOCUMENT_API_KEY=provider_key,
                   OHAGENT_DOCUMENT_BASE_URL=args.provider_base_url, OHAGENT_DOCUMENT_MODEL=args.model,
                   OHAGENT_DOCUMENT_JSON_MODE="json_object", OHAGENT_DOCUMENT_BIND=f"127.0.0.1:{core_port}")
        core = subprocess.Popen([str(Path(args.ohagent_binary).resolve())], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        processes.append(core)
        wait_ready(core, ("127.0.0.1", core_port))
        safe_gateway_env = {name: value for name, value in os.environ.items()
                            if name not in (args.key_env, "OHAGENT_DOCUMENT_API_KEY", "VAULT_TOKEN")}
        gateway_env = dict(safe_gateway_env, DATABASE_URL=f"postgres://document_test@127.0.0.1:{args.db_port}/document_test?sslmode=disable&search_path={schema}",
                           JWT_SECRET=jwt_secret, GATEWAY_BIND=f"127.0.0.1:{gateway_port}",
                           OHAGENT_DOCUMENT_URL=f"http://127.0.0.1:{core_port}/api/document-processing/extract", OHAGENT_API_KEY=service_key)
        gateway = subprocess.Popen([str(Path(args.gateway_binary).resolve()), "-port", str(gateway_port)], env=gateway_env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        processes.append(gateway)
        wait_ready(gateway, ("127.0.0.1", gateway_port))
        base = f"http://127.0.0.1:{gateway_port}"
        tokens = {tenant_a: token(jwt_secret, tenant_a), tenant_b: token(jwt_secret, tenant_b)}

        def request(tenant, method, path, data=None, expected=200):
            body = json.dumps(data).encode() if data is not None else None
            headers = {"Content-Type": "application/json", "X-Tenant-Id": tenant_b}
            if tenant:
                headers["Authorization"] = "Bearer " + tokens[tenant]
            req = urllib.request.Request(base + path, data=body, headers=headers, method=method)
            try:
                with urllib.request.urlopen(req, timeout=120) as response:
                    status, content = response.status, response.read()
            except urllib.error.HTTPError as error:
                status, content = error.code, error.read()
            assert status == expected, f"{method} {path}: HTTP {status}, expected {expected}, body {content[:1000]!r}"
            return json.loads(content) if content and content[:1] in (b"{", b"[") else content

        image = Image.new("RGB", (1200, 650), "white")
        draw = ImageDraw.Draw(image)
        font = ImageFont.truetype("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", 30)
        for index, line in enumerate(["ARTIFICIAL TEST INVOICE", "Invoice number: TEST-2026-001", "Issue date: 2026-11-03", "Supplier: Artificial Supplies SIA", "Supplier registration number: 40000000000", "Total amount: 123.45 EUR"]):
            draw.text((40, 40 + index * 75), line, font=font, fill="black")
        stream = io.BytesIO()
        image.save(stream, format="PNG")
        upload = {"file_name": "artificial-acceptance.png", "media_type": "image/png", "content_base64": base64.b64encode(stream.getvalue()).decode()}
        request(None, "GET", "/api/v1/document-jobs", expected=401)
        job = request(tenant_a, "POST", "/api/v1/document-jobs", upload, expected=201)
        assert job["state"] == "review_required", job
        extraction = job["extraction"]
        assert extraction["document_type"] == "invoice"
        assert extraction["fields"]["invoice_number"] == "TEST-2026-001"
        assert extraction["fields"]["total_amount"] == "123.45"
        assert extraction["fields"]["currency"] == "EUR"
        path = "/api/v1/document-jobs/" + job["id"]
        request(tenant_a, "POST", path + "/apply", {"version": job["version"]}, expected=409)
        request(tenant_b, "GET", path, expected=404)
        original = request(tenant_a, "GET", path + "/original")
        assert original == stream.getvalue(), "Original bytes changed or were lost"
        request(tenant_b, "GET", path + "/original", expected=404)
        targets = request(tenant_a, "GET", path + "/targets")
        assert supplier_b not in json.dumps(targets)
        assert supplier_a in json.dumps(targets)
        review = {"version": job["version"], "document_type": "invoice", "fields": extraction["fields"], "mapping": {"supplier_id": supplier_b, "debit_account": "6300", "credit_account": "2110"}}
        request(tenant_a, "POST", path + "/review", review, expected=422)
        review["mapping"]["supplier_id"] = supplier_a
        reviewed = request(tenant_a, "POST", path + "/review", review)
        applied = request(tenant_a, "POST", path + "/apply", {"version": reviewed["version"]})
        repeated = request(tenant_a, "POST", path + "/apply", {"version": reviewed["version"]})
        assert applied["applied_transaction_id"] == repeated["applied_transaction_id"]
        duplicate = request(tenant_a, "POST", "/api/v1/document-jobs", upload)
        assert duplicate["id"] == job["id"]
        transaction = request(tenant_a, "GET", "/api/v1/transactions/" + applied["applied_transaction_id"])
        assert job["id"] in json.dumps(transaction)
        request(tenant_b, "GET", "/api/v1/transactions/" + applied["applied_transaction_id"], expected=404)
        assert request(tenant_b, "GET", "/api/v1/journal") == []
        totals = scoped(f"SELECT count(*),SUM(e.debit_amount)::text,SUM(e.credit_amount)::text FROM transaction_entries e JOIN transactions t ON t.id=e.transaction_id WHERE t.source_document_job_id='{job['id']}';").splitlines()[-1]
        assert totals == "2|123.45|123.45", totals
        print(json.dumps({"status": "PASS", "model": extraction["model"], "workflow": "web API upload -> live vision -> classification -> tenant targets -> human review -> real balanced posting", "checked_amount": "123.45 EUR", "source_retained": True, "duplicate_effects": False, "cross_tenant_access": "rejected", "unreviewed_apply": "rejected"}), flush=True)
        if args.browser_seconds:
            class Bootstrap(BaseHTTPRequestHandler):
                def log_message(self, *_):
                    pass

                def do_GET(self):
                    if self.path != "/synthetic-browser-bootstrap":
                        self.send_error(404)
                        return
                    payload = json.dumps({"token": tokens[tenant_a], "tenant_id": tenant_a,
                                          "gateway_url": base}).encode()
                    self.send_response(200)
                    self.send_header("Content-Type", "application/json")
                    self.send_header("Cache-Control", "no-store")
                    self.send_header("Access-Control-Allow-Origin", "http://127.0.0.1:3302")
                    self.send_header("Content-Length", str(len(payload)))
                    self.end_headers()
                    self.wfile.write(payload)

            bootstrap = ThreadingHTTPServer(("127.0.0.1", 0), Bootstrap)
            thread = threading.Thread(target=bootstrap.serve_forever, daemon=True)
            thread.start()
            print(json.dumps({"message": "Synthetic browser fixture ready", "gateway_url": base,
                  "bootstrap_url": f"http://127.0.0.1:{bootstrap.server_port}/synthetic-browser-bootstrap",
                  "lifetime_seconds": args.browser_seconds}), flush=True)
            try:
                threading.Event().wait(args.browser_seconds)
            finally:
                bootstrap.shutdown()
                bootstrap.server_close()
                thread.join(timeout=5)
    finally:
        for process in reversed(processes):
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
        sql(f"DROP SCHEMA IF EXISTS {schema} CASCADE;")


if __name__ == "__main__":
    main()
