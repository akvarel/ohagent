#!/usr/bin/env python3
"""Three explicit local HTTP fixtures, never an OCR-quality or live-vendor claim.

Only synthetic acceptance scripts use this helper. Provider credentials and document
contents are never printed. Each endpoint implements the real chat-completions wire
boundary so acceptance exercises the real restricted client's HTTP and validation.
"""
from contextlib import AbstractContextManager
from datetime import date
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import threading
import uuid


class ComparisonFixture(AbstractContextManager):
    def __init__(self):
        self.servers = []
        self.threads = []
        self.calls = []
        self.lock = threading.Lock()
        self.kind = "invoice"
        self.invoice_number = "TEST-2026-001"
        self.meter_number = "DOC-LIVE-MTR-001"
        self.reading_value = "160.00"
        self.third_disagrees = False
        self.failed_slots = set()
        self.admin_key = "artificial-admin-" + uuid.uuid4().hex

    def __enter__(self):
        for index in range(3):
            fixture = self
            slot = index

            class Handler(BaseHTTPRequestHandler):
                fixture_slot = slot

                def log_message(self, *_):
                    pass

                def do_POST(self):
                    if self.path != "/v1/chat/completions":
                        self.send_error(404)
                        return
                    length = int(self.headers.get("Content-Length", "0"))
                    if length <= 0 or length > 20 * 1024 * 1024:
                        self.send_error(413)
                        return
                    request = json.loads(self.rfile.read(length))
                    model = request.get("model")
                    with fixture.lock:
                        fixture.calls.append({"slot": self.fixture_slot, "model": model})
                    if self.fixture_slot in fixture.failed_slots:
                        self.send_error(503, "Artificial provider failure")
                        return
                    fields = dict.fromkeys(("invoice_number", "issue_date", "currency", "total_amount",
                        "supplier_name", "supplier_registration_number", "meter_number", "reading_value",
                        "reading_date", "property_address", "unit_number"))
                    fields["property_address"] = "1 Test Street, Riga"
                    if fixture.kind == "invoice":
                        fields.update(invoice_number=fixture.invoice_number, issue_date="2026-11-03", currency="EUR",
                            total_amount="124.45" if fixture.third_disagrees and self.fixture_slot == 2 else "123.45",
                            supplier_name="Artificial Supplies SIA", supplier_registration_number="40000000000")
                    else:
                        fields.update(meter_number=fixture.meter_number, reading_value=fixture.reading_value,
                            reading_date=date.today().isoformat(), unit_number="1")
                    extraction = {"document_type": fixture.kind, "fields": fields, "evidence": [],
                        "warnings": ["Artificial HTTP provider fixture, not live OCR"], "model": "untrusted-model-label"}
                    payload = json.dumps({"choices": [{"finish_reason": "stop", "message": {
                        "role": "assistant", "content": json.dumps(extraction)}}]}).encode()
                    self.send_response(200)
                    self.send_header("Content-Type", "application/json")
                    self.send_header("Content-Length", str(len(payload)))
                    self.end_headers()
                    self.wfile.write(payload)

            server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            self.servers.append(server)
            self.threads.append(thread)
        return self

    def environment(self, directory, pdf=False, render_image="docker.io/library/alpine:3.20"):
        profiles = []
        environment = {"OHAGENT_DOCUMENT_ADMIN_KEY": self.admin_key,
            "OHAGENT_DOCUMENT_MODELS_FILE": str(directory) + "/document-models.json"}
        for index, server in enumerate(self.servers):
            reference = "ARTIFICIAL_DOCUMENT_PROFILE_KEY_" + str(index)
            environment[reference] = "artificial-profile-key-" + uuid.uuid4().hex
            profile = {"id": "fixture-" + str(index + 1), "label": "Artificial provider " + str(index + 1),
                "base_url": f"http://127.0.0.1:{server.server_port}/v1", "api_key_env": reference,
                "model": f"fixture-model-{index + 1}-v1", "enabled": True, "json_mode": "json_object",
                "pdf_mode": "podman" if pdf else "native"}
            if pdf:
                profile["render_image"] = render_image
            profiles.append(profile)
        environment["OHAGENT_DOCUMENT_PROFILES"] = json.dumps(profiles)
        return environment

    def __exit__(self, *_):
        for server in self.servers:
            server.shutdown()
            server.server_close()
        for thread in self.threads:
            thread.join(timeout=5)
        return False
