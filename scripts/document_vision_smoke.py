#!/usr/bin/env python3
"""Opt-in live vision smoke using generated, non-customer documents.

Requires Pillow and an existing provider key. Never prints credentials or changes
persistent provider settings. This is not an accuracy benchmark for handwriting.
"""
import argparse
import base64
import io
import json
import os
from pathlib import Path
import socket
import subprocess
import time
import urllib.error
import urllib.request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--base-url", required=True)
    parser.add_argument("--model", required=True)
    parser.add_argument("--key-env", required=True)
    parser.add_argument("--pdf", action="store_true", help="Render generated PDFs in the isolated worker")
    parser.add_argument("--render-image", default="docker.io/library/alpine:3.20")
    args = parser.parse_args()
    key = os.environ.get(args.key_env)
    if not key:
        raise SystemExit("Configured provider key is unavailable")
    from PIL import Image, ImageDraw, ImageFont

    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    service_key = "artificial-local-smoke-service-key"
    env = dict(os.environ, OHAGENT_API_KEY=service_key,
               OHAGENT_DOCUMENT_API_KEY=key,
               OHAGENT_DOCUMENT_BASE_URL=args.base_url,
               OHAGENT_DOCUMENT_MODEL=args.model,
               OHAGENT_DOCUMENT_JSON_MODE="json_object",
               OHAGENT_DOCUMENT_BIND=f"127.0.0.1:{port}")
    if args.pdf:
        env.update(OHAGENT_DOCUMENT_PDF_MODE="podman", OHAGENT_DOCUMENT_RENDER_IMAGE=args.render_image)
    process = subprocess.Popen([str(Path(args.binary).resolve())], env=env,
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        for _ in range(100):
            if process.poll() is not None:
                raise RuntimeError("Restricted service exited before readiness")
            try:
                with socket.create_connection(("127.0.0.1", port), timeout=0.1):
                    break
            except OSError:
                time.sleep(0.05)
        else:
            raise RuntimeError("Restricted service readiness timed out")
        examples = [
            ("invoice", ["ARTIFICIAL TEST INVOICE", "Invoice number: TEST-2026-001",
                         "Issue date: 2026-11-03", "Supplier: Artificial Supplies SIA",
                         "Supplier registration number: 40000000000", "Total amount: 123.45 EUR"],
             {"invoice_number": "TEST-2026-001", "total_amount": "123.45", "currency": "EUR"}),
            ("meter_reading", ["ARTIFICIAL METER READING REPORT", "Meter serial number: WATER-001",
                               "Reading date: 2026-11-03", "Reading value: 1234.56 m3",
                               "Property address: Example Street 10", "Apartment number: 4"],
             {"meter_number": "WATER-001", "reading_value": "1234.56", "reading_date": "2026-11-03"}),
        ]
        font = ImageFont.truetype("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", 30)
        for kind, lines, expected in examples:
            image = Image.new("RGB", (1200, 650), "white")
            draw = ImageDraw.Draw(image)
            for index, line in enumerate(lines):
                draw.text((40, 40 + 75 * index), line, font=font, fill="black")
            stream = io.BytesIO()
            image.save(stream, format="PDF" if args.pdf else "PNG")
            extension, media_type = ("pdf", "application/pdf") if args.pdf else ("png", "image/png")
            payload = json.dumps({"file_name": f"artificial-{kind}.{extension}", "media_type": media_type,
                                  "content_base64": base64.b64encode(stream.getvalue()).decode()}).encode()
            request = urllib.request.Request(f"http://127.0.0.1:{port}/api/document-processing/extract", data=payload,
                headers={"Content-Type": "application/json", "Authorization": f"Bearer {service_key}",
                         "X-Tenant-Id": "artificial-smoke-tenant"})
            try:
                with urllib.request.urlopen(request, timeout=100) as response:
                    result = json.load(response)
            except urllib.error.HTTPError as error:
                safe = error.read(2048).decode(errors="replace")
                raise RuntimeError(f"Extraction failed HTTP {error.code}: {safe}") from None
            assert result["document_type"] == kind, f"Incorrect classification: {result['document_type']}"
            for field, value in expected.items():
                assert result["fields"][field] == value, f"Incorrect {field}: {result['fields'][field]!r}"
            print(json.dumps({"status": "PASS", "document_type": kind, "model": result["model"],
                              "media_type": media_type,
                              "checked_fields": expected, "evidence_count": len(result["evidence"]),
                              "warning_count": len(result["warnings"])}), flush=True)
        print("PASS: two generated documents traversed actual restricted HTTP service and live vision provider")
    finally:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)


if __name__ == "__main__":
    main()
