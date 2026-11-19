#!/usr/bin/env python3
"""Loopback-only browser acceptance for the real monitor binary and built dashboard.

Uses synthetic fixture tokens, never production credentials. Requires Playwright
and an installed Chromium. No provider calls, agent turns or remote deployment.
"""
import argparse
import json
import os
from pathlib import Path
import socket
import subprocess
import time
import urllib.error
import urllib.request

from playwright.sync_api import sync_playwright, expect

ROOT = Path(__file__).resolve().parents[1]
VIEW = "fixture-read-token-not-a-real-secret-0001"
PUBLISH = "fixture-write-token-not-a-real-secret-001"


def request(base, path, token=None, data=None):
    headers = {"Authorization": f"Bearer {token}"} if token else {}
    if data is not None:
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(base + path, headers=headers,
                                 data=json.dumps(data).encode() if data is not None else None)
    try:
        with urllib.request.urlopen(req, timeout=3) as response:
            return response.status, response.headers, response.read()
    except urllib.error.HTTPError as error:
        return error.code, error.headers, error.read()


def fixture(status="working", sequence=1):
    now = int(time.time() * 1000)
    return {"version": 1, "tenant_id": "acceptance", "source_id": "workstation",
            "boot_id": "browser-fixture", "sequence": sequence, "generated_at": now,
            "sessions": [{"id": "a" * 24, "status": status, "updated_at": now,
                          "started_at": now - 10000, "tools": {"read": 7, "bash": 2},
                          "current_tool": "read" if status == "working" else None,
                          "input_tokens": 12400, "output_tokens": 350,
                          "events": [{"at": now, "kind": status, "tool": None}]}]}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--screenshots", type=Path, help="Optional directory for synthetic-data screenshots")
    args = parser.parse_args()
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    base = f"http://127.0.0.1:{port}"
    env = {**os.environ, "OHAGENT_PROGRESS_BIND": f"127.0.0.1:{port}",
           "OHAGENT_PROGRESS_TENANT_ID": "acceptance", "OHAGENT_PROGRESS_VIEW_TOKEN": VIEW,
           "OHAGENT_PROGRESS_PUBLISH_TOKEN": PUBLISH,
           "OHAGENT_PROGRESS_WEB_ROOT": str(ROOT / "target/dashboard-dist")}
    server = subprocess.Popen([str(args.binary.resolve())], env=env, cwd=ROOT,
                              stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    try:
        for _ in range(100):
            if server.poll() is not None:
                raise RuntimeError(server.stderr.read().decode())
            try:
                if request(base, "/progress")[0] == 200:
                    break
            except (OSError, urllib.error.URLError):
                time.sleep(0.05)
        else:
            raise RuntimeError("Monitor did not start")
        code, headers, _ = request(base, "/progress")
        assert code == 200 and headers["Cache-Control"] == "no-store"
        assert "frame-ancestors 'none'" in headers["Content-Security-Policy"]
        assert request(base, "/api/keys")[0] == 404
        assert request(base, "/api/progress?tenant_id=acceptance")[0] == 401
        assert request(base, "/api/progress?tenant_id=other", VIEW)[0] == 403
        assert request(base, "/api/progress/snapshots", VIEW, fixture())[0] == 401
        assert request(base, "/api/progress/snapshots", PUBLISH, fixture())[0] == 204
        with sync_playwright() as p:
            browser = p.chromium.launch(headless=True)
            context = browser.new_context(viewport={"width": 1280, "height": 900}, locale="en-US")
            page = context.new_page()
            errors = []
            page.on("pageerror", lambda e: errors.append(str(e)))
            page.goto(base + "/progress")
            expect(page.get_by_role("heading", name="Work in view")).to_be_visible()
            page.get_by_label("Tenant ID", exact=True).fill("acceptance")
            page.get_by_label("Read token", exact=True).fill(VIEW)
            page.get_by_role("button", name="Connect", exact=True).click()
            expect(page.locator(".progress-connection")).to_contain_text("Connected")
            expect(page.get_by_role("heading", name="workstation")).to_be_visible()
            expect(page.locator(".progress-current strong")).to_have_text("read")
            if args.screenshots:
                args.screenshots.mkdir(parents=True, exist_ok=True)
                page.screenshot(path=str(args.screenshots / "desktop.png"), full_page=True)
            assert page.evaluate("localStorage.length + sessionStorage.length") == 0
            page.get_by_role("button", name="Needs attention").click()
            expect(page.get_by_text("No sessions match this filter.")).to_be_visible()
            assert request(base, "/api/progress/snapshots", PUBLISH, fixture("waiting", 2))[0] == 204
            expect(page.locator(".progress-status")).to_have_text("Waiting for permission", timeout=8000)
            page.get_by_role("combobox", name="Language", exact=True).select_option("ru")
            expect(page.get_by_role("heading", name="Работа на виду")).to_be_visible()
            page.get_by_role("combobox", name="Язык", exact=True).select_option("lv")
            expect(page.get_by_role("heading", name="Darbs redzeslokā")).to_be_visible()
            page.get_by_role("combobox", name="Valoda", exact=True).select_option("en")
            page.get_by_role("button", name="All sessions").click()
            page.set_viewport_size({"width": 390, "height": 844})
            assert page.evaluate("document.documentElement.scrollWidth <= innerWidth"), "mobile overflow"
            if args.screenshots:
                page.screenshot(path=str(args.screenshots / "mobile.png"), full_page=True)
            page.route("**/api/progress?*", lambda route: route.abort())
            expect(page.get_by_role("alert")).to_contain_text("Cannot reach", timeout=15000)
            expect(page.get_by_role("heading", name="workstation")).to_be_visible()
            page.unroute("**/api/progress?*")
            expect(page.get_by_role("alert")).to_have_count(0, timeout=15000)
            # Browser clock offset proves the offline label updates without new data.
            page.evaluate("Date.now = () => new Date().getTime() + 100000")
            expect(page.get_by_text("Publisher offline", exact=True)).to_be_visible(timeout=5000)
            page.get_by_role("button", name="Disconnect", exact=True).click()
            expect(page.get_by_text("Not connected", exact=True)).to_be_visible()
            expect(page.get_by_label("Read token", exact=True)).to_have_value("")
            # Access failure is rendered, not an empty dashboard or infinite retries.
            page.get_by_label("Read token", exact=True).fill("invalid-fixture-token")
            page.get_by_role("button", name="Connect", exact=True).click()
            expect(page.get_by_role("alert")).to_contain_text("Access denied")
            assert not errors, errors
            browser.close()
        print("PASS: real receiver HTTP auth/scope/headers, browser live updates, filters, EN/RU/LV, mobile, stale state, disconnect and denied access")
    finally:
        server.terminate()
        try:
            server.wait(timeout=5)
        except subprocess.TimeoutExpired:
            server.kill()
            server.wait()
        if server.stderr:
            server.stderr.close()


if __name__ == "__main__":
    main()
