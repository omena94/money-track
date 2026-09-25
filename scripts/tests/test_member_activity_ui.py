"""Synthetic HTTP + real React pages in isolated Edge; no real account or database."""
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import socket
import subprocess
import threading
import time
from DrissionPage import Chromium, ChromiumOptions
from edge_profile import isolate

ROOT = Path(__file__).resolve().parents[2]


def wait_for(check, message):
    until = time.monotonic() + 5
    while time.monotonic() < until:
        if check():
            return
        time.sleep(0.05)
    raise AssertionError(message)


def main():
    subprocess.run(['node', 'scripts/tests/build-member-activity-ui.mjs'], cwd=ROOT, check=True)
    class QuietHandler(SimpleHTTPRequestHandler):
        def log_message(self, *_):
            pass
    server = ThreadingHTTPServer(('127.0.0.1', 0), partial(QuietHandler, directory=str(ROOT / '.local/member-activity-ui')))
    threading.Thread(target=server.serve_forever, daemon=True).start()
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        port = sock.getsockname()[1]
    options = ChromiumOptions(read_file=False).set_browser_path(r'C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe')
    options.set_user_data_path(str(ROOT / '.local/member-activity-ui/profile')).set_local_port(port).headless(True)
    browser = None
    try:
        browser = Chromium(isolate(options))
        tab = browser.latest_tab
        tab.get(f'http://127.0.0.1:{server.server_port}/')
        cell = lambda: tab.run_js('return document.querySelector(".member-cell")?.textContent || ""')
        wait_for(lambda: '4 小時前' in cell(), 'initial activity not rendered')
        tab.run_js('fixture.activity = new Date(fixture.now).toISOString()')
        wait_for(lambda: '剛剛' in cell(), 'member activity did not refresh without navigation')
        tab.run_js('fixture.now += 120000')
        wait_for(lambda: '2 分鐘前' in cell(), 'relative time froze for identical API responses')
        wait_for(lambda: tab.run_js('return fixture.calls["/auth/me"] > 1'), 'visible page did not heartbeat')
        tab.run_js('fixture.failMe = true')
        time.sleep(0.7)
        assert '2 分鐘前' in cell(), 'temporary heartbeat failure removed workspace'
        tab.run_js('fixture.failMe = false; window.setFocused(false)')
        time.sleep(0.4)
        calls = tab.run_js('return JSON.stringify(fixture.calls)')
        time.sleep(0.6)
        assert calls == tab.run_js('return JSON.stringify(fixture.calls)'), 'background page kept polling'
        tab.run_js('fixture.activity = new Date(fixture.now).toISOString(); window.setFocused(true)')
        wait_for(lambda: '剛剛' in cell(), 'returning to page did not refresh')
        tab.run_js('fixture.activity = null')
        wait_for(lambda: '尚無活動紀錄' in cell(), 'unknown activity incorrectly shown as never logged in')
        tab.run_js('fixture.legacy = true; fixture.activity = new Date(fixture.now).toISOString()')
        wait_for(lambda: tab.run_js('return !!document.querySelector("[data-label=最近登入]")'), 'old API login time mislabeled as activity')
        tab.run_js('fixture.legacy = false')
        wait_for(lambda: tab.run_js('return !!document.querySelector("[data-label=最近活動]")'), 'new API did not enable activity display')
        tab.run_js('fixture.unauthorized = true')
        wait_for(lambda: tab.run_js('return !!document.querySelector(".login")'), 'expired session did not return to login')
        time.sleep(0.4)
        calls = tab.run_js('return JSON.stringify(fixture.calls)')
        time.sleep(0.6)
        assert calls == tab.run_js('return JSON.stringify(fixture.calls)'), 'expired session kept polling'
        print('PASS: live refresh, relative clock, heartbeat, transient error, background pause, focus refresh, unknown activity, expired session')
    finally:
        if browser:
            browser.quit()
        server.shutdown()


if __name__ == '__main__':
    main()
