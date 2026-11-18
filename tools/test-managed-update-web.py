#!/usr/bin/env python3
"""Exercise the real remote Web About page against an isolated managed host."""
import argparse
import json
import os
from pathlib import Path
import shutil
from playwright.sync_api import expect, sync_playwright


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--origin", default="http://127.0.0.1:18888")
    parser.add_argument("--expected", choices=["installed", "rolled_back", "failed"], required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    args = parser.parse_args()
    chrome = os.environ.get("CHROME_PATH") or shutil.which("google-chrome") or shutil.which("chromium")
    if not chrome and os.name == "nt":
        chrome = "C:/Program Files/Google/Chrome/Application/chrome.exe"
    with sync_playwright() as playwright:
        browser = playwright.chromium.launch(**({"executable_path": chrome} if chrome else {}), headless=True, args=["--mute-audio"])
        context = browser.new_context(viewport={"width": 1280, "height": 960})
        page = context.new_page()
        page.set_default_timeout(180000)
        errors, phases = [], []
        page.on("pageerror", lambda error: errors.append(str(error)))
        def observe(response):
            if response.url.endswith("/api/v1/host/software") and response.status == 200:
                try:
                    phase = response.json()["phase"]
                    if not phases or phases[-1] != phase:
                        phases.append(phase)
                except Exception:
                    pass
        page.on("response", observe)
        page.goto(args.origin + "/about", wait_until="domcontentloaded")
        expect(page.get_by_role("heading", name="Software updates", exact=True)).to_be_visible()
        expect(page.locator(".about-version")).to_contain_text("0.1.39")
        page.get_by_role("button", name="Check for updates", exact=True).click()
        expect(page.get_by_role("button", name="Install and restart", exact=True)).to_be_enabled()
        before = context.request.get(args.origin + "/api/v1/host/software").json()
        assert before["phase"] == "available" and before["can_install"], before
        page.get_by_role("button", name="Install and restart", exact=True).click()
        expect(page.get_by_role("group", name="Confirm installation")).to_be_visible()
        page.get_by_role("button", name="Cancel", exact=True).click()
        assert context.request.get(args.origin + "/api/v1/host/software").json()["phase"] == "available"
        page.get_by_role("button", name="Install and restart", exact=True).click()
        with page.expect_response(lambda response: response.url.endswith("/api/v1/host/updates/install")) as started:
            page.get_by_role("button", name="Install now", exact=True).click()
        assert started.value.status == 200, started.value.text()
        phrases = {"installed": "was installed and its services passed", "rolled_back": "Previous RackForge 0.1.39 restored and checked", "failed": "Archive digest does not match"}
        expect(page.locator(".software-update-status")).to_contain_text(phrases[args.expected], timeout=180000)
        state = context.request.get(args.origin + "/api/v1/host/software").json()
        assert state["phase"] == args.expected, state
        expect(page.locator(".about-version")).to_contain_text(state["current_version"])
        assert not errors, errors
        args.receipt.parent.mkdir(parents=True, exist_ok=True)
        page.screenshot(path=str(args.receipt.with_suffix(".png")), full_page=True)
        args.receipt.write_text(json.dumps(dict(expected=args.expected, state=state, phases=phases, confirmation_cancel="preserved offer", browser_errors=errors), indent=2))
        print(args.receipt.read_text(), flush=True)
        context.close(); browser.close()


if __name__ == "__main__":
    main()
