"""分類規則 UI 驗收；只使用短暫測試身分，憑證保留在記憶體。"""
import json
import os
import socket
import subprocess
import time
from urllib.parse import urlparse

from DrissionPage import Chromium, ChromiumOptions
from test_ui import ROOT, FIXTURE, OUTPUT, isolate, pick, override_api_origin


def set_period(tab, label, value):
    # Chromium 的日期／月份原生輸入不接受一般貼上；透過原生 setter 觸發 React change。
    tab.run_js("""const input = document.querySelector(`input[aria-label="${arguments[0]}"]`);
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, arguments[1]);
        input.dispatchEvent(new Event('input', {bubbles: true}));
        input.dispatchEvent(new Event('change', {bubbles: true}));""", label, value)


def main():
    identity = json.loads(subprocess.run(
        [str(FIXTURE)], capture_output=True, check=True, encoding="utf-8").stdout)
    browser = None
    try:
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        options = ChromiumOptions(read_file=False).set_browser_path(
            r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe")
        options.set_user_data_path(str(ROOT / ".local/categories-ui-profile")).set_local_port(port).headless(False)
        options.set_argument("--window-size", "1440,1050")
        isolate(options)
        browser = Chromium(options)
        tab = browser.latest_tab
        origin = os.environ.get("APP_ORIGIN", "http://localhost:3000")
        tab.get(origin)
        if os.environ.get("API_ORIGIN"):
            override_api_origin(tab, os.environ["API_ORIGIN"])
        tab.run_cdp("Network.setCookie", name="money_session", value=identity["session"],
                    domain=urlparse(origin).hostname, path="/", httpOnly=True,
                    secure=origin.startswith("https"), sameSite="Lax")
        tab.refresh()
        assert tab.ele("css:.greet", timeout=15)
        tab.ele('css:.sidebar a[href="#/activity"]').click()
        pick(tab, "button", "分類規則").click()
        dialog = tab.ele('css:[role="dialog"]', timeout=10)
        assert pick(dialog, "h3", "預設規則")
        search = dialog.ele('css:input[placeholder^="搜尋分類"]')
        search.input("全聯")
        assert pick(dialog, ".default-rule-groups", "全聯")
        assert "僅支出" in dialog.ele("css:.default-rule-groups").text
        search.clear()
        # 儲存個人規則，顯示在我的規則清單；API 實際寫入測試帳本。
        dialog.ele('css:input[placeholder^="例如："]').input("測試商家")
        pick(dialog, ".cat-grid button", "購物", exact=True).click()
        pick(dialog, "button", "新增規則", exact=True).click()
        assert pick(dialog, ".rule-list", "測試商家")
        form = dialog.ele("css:.reclassify-form")
        select = form.ele("tag:select")
        select.select.by_value("months")
        set_period(tab, "開始期間", "2026-08")
        set_period(tab, "結束期間", "2026-09")
        pick(form, "button", "依目前規則重新分類").click()
        assert "2026-08 至 2026-09" in form.ele('css:[role="status"]', timeout=10).text
        assert "已重新分類 0 筆" in form.ele('css:[role="status"]').text
        select.select.by_value("dates")
        set_period(tab, "開始期間", "2026-09-01")
        set_period(tab, "結束期間", "2026-09-30")
        pick(form, "button", "依目前規則重新分類").click()
        assert "2026-09-01 至 2026-09-30" in form.ele('css:[role="status"]', timeout=10).text
        form.scroll.to_see()
        tab.get_screenshot(path=OUTPUT / "categories-desktop.png")
        # 手機日期欄位與規則展開，不產生水平捲動。
        tab.run_cdp("Emulation.setDeviceMetricsOverride", width=390, height=844, deviceScaleFactor=1, mobile=True)
        time.sleep(0.3)
        assert tab.run_js("return document.documentElement.scrollWidth <= window.innerWidth")
        assert tab.run_js("return [...document.querySelectorAll('.reclassify-dates input')].every(e => e.getBoundingClientRect().right <= window.innerWidth)")
        tab.get_screenshot(path=OUTPUT / "categories-mobile.png")
        select.select.by_value("all")
        pick(form, "button", "依目前規則重新分類").click()
        assert "全部期間" in form.ele('css:[role="status"]', timeout=10).text
        dialog.ele('css:button[aria-label="刪除規則「測試商家」"]').click()
        assert pick(dialog, ".empty strong", "還沒有自己的規則")
        print("Category UI passed: default search/conditions, personal create/delete, all/month/date reclassification, result counts, desktop/mobile layout. Synthetic user only.")
    finally:
        if browser:
            browser.quit()
        subprocess.run([str(FIXTURE), identity["id"]], capture_output=True, check=True)


if __name__ == "__main__":
    main()
