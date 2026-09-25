"""Visible Edge integration test, isolated synthetic user. Secrets remain in memory."""
import json
import os
from pathlib import Path
import socket
import time
import subprocess
import sys
from urllib.parse import urlparse
from DrissionPage import Chromium, ChromiumOptions
from edge_profile import isolate  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
FIXTURE = ROOT / 'target/debug/examples/ui_fixture.exe'
OUTPUT = ROOT / '.local/ui'
OUTPUT.mkdir(parents=True, exist_ok=True)
DIALOG = 'css:[role="dialog"]'


def main():
    fixture = subprocess.run([str(FIXTURE)], capture_output=True, check=True, encoding='utf-8')
    identity = json.loads(fixture.stdout)
    browser = None
    try:
        with socket.socket() as s:
            s.bind(('127.0.0.1', 0))
            port = s.getsockname()[1]
        options = ChromiumOptions(read_file=False).set_browser_path(r'C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe')
        options.set_user_data_path(str(ROOT / '.local/ui-profile')).set_local_port(port).headless(False)
        options.set_argument('--window-size', '1440,1050')
        isolate(options)
        browser = Chromium(options)
        tab = browser.latest_tab
        origin = os.environ.get('APP_ORIGIN', 'http://localhost:3000')
        tab.get(origin)
        tab.run_js('localStorage.clear()')
        # 測試非 3000 埠的開發伺服器時，API 只接受設定的正式 Origin；以 API_ORIGIN 明確指定。
        if os.environ.get('API_ORIGIN'):
            override_api_origin(tab, os.environ['API_ORIGIN'])
        tab.run_cdp('Network.setCookie', name='money_session', value=identity['session'], domain=urlparse(origin).hostname, path='/', httpOnly=True, secure=origin.startswith('https'), sameSite='Lax')
        tab.refresh()
        assert tab.ele('css:.greet', timeout=15)
        assert '開始使用' not in tab.ele('tag:main').text
        shot(tab, 'overview-empty-desktop')

        def nav(label):
            tab.ele(f'css:.sidebar a.nav-item[href="#/{label}"]').click()

        def dialog():
            return tab.ele(DIALOG, timeout=10)

        def closed():
            assert tab.wait.ele_deleted(DIALOG, timeout=15), 'dialog stayed open'

        # 沒有帳戶時，記一筆會先引導建立帳戶，建立後直接回到記帳
        tab.ele('css:.compose-btn').click()
        d = dialog()
        assert '先建立第一個帳戶' in d.text
        pick(d, '.kind-option', '銀行帳戶').click()
        d.ele('css:input[name=name]').input('測試生活帳戶')
        d.ele('css:input[name=opening_balance]').input('50000')
        d.ele('css:button[type=submit]').click()
        assert tab.wait.ele_displayed('css:#composer', timeout=10)

        # 算式金額、分類、儲存
        d = dialog()
        d.ele('css:input[name=amount]').input('100+20')
        assert 'NT$ 120' in d.ele('#amount-help').text
        d.ele('css:input[name=description]').input('測試午餐')
        pick(d, '.category-option', '餐飲').click()
        d.ele('css:button[form=composer][type=submit]').click()
        closed()
        assert tab.wait.ele_displayed('text:已記下「測試午餐」', timeout=10)
        assert tab.wait.ele_displayed('text:49,880', timeout=15)

        # 明細抽屜：編輯
        nav('activity')
        pick(tab, 'button.row', '測試午餐').click()
        d = dialog()
        pick(d, '.action-list button', '編輯分類').click()
        d.ele('css:input[name=tags]').input('家庭,餐點')
        d.ele('css:textarea[name=note]').input('可見瀏覽器驗證')
        d.ele('css:button[type=submit]').click()
        closed()

        # 拆帳：填入剩餘
        pick(tab, 'button.row', '測試午餐').click()
        d = dialog()
        pick(d, '.action-list button', '拆帳').click()
        parts = d.eles('css:.split-part')
        parts[0].ele('css:input[inputmode=decimal]').input('80')
        parts[1].ele('css:.affix-action').click()
        assert '已全部分配' in d.ele('css:.split-status').text
        d.ele('css:button[type=submit]').click()
        closed()
        assert pick(tab, '.badge', '拆帳', exact=True)
        body = tab.ele('tag:body').text
        assert '−NT$ 80' in body and '−NT$ 40' in body, 'split rows missing'

        # 退款
        pick(tab, 'button.row', '測試午餐').click()
        d = dialog()
        pick(d, '.action-list button', '記錄退款').click()
        d.ele('css:input[name=amount]').input('20')
        d.ele('css:button[type=submit]').click()
        closed()
        assert tab.wait.ele_displayed('text:退款 · 測試午餐', timeout=15)
        nav('overview')
        assert tab.wait.ele_displayed('text:49,900', timeout=15)
        assert tab.ele('css:.worth .trend', timeout=10), 'net worth trend missing'
        shot(tab, 'dashboard-desktop')

        nav('assets')
        assert tab.ele('text:測試生活帳戶', timeout=10)
        shot(tab, 'assets-desktop')

        # 預算
        nav('plans')
        pick(tab, '.month-nav button', '設定預算').click()
        d = dialog()
        pick(d, '.category-option', '餐飲').click()
        d.ele('css:input[name=amount]').input('6000')
        d.ele('css:button[type=submit]').click()
        closed()
        assert tab.wait.ele_displayed('css:.budget', timeout=10)
        assert '6,000' in tab.ele('css:.budget').text

        # 目標與存入
        pick(tab, '.segmented button', '儲蓄目標', exact=True).click()
        pick(tab, '.month-nav button', '新增目標').click()
        d = dialog()
        d.ele('css:input[name=name]').input('測試旅行')
        d.ele('css:input[name=target]').input('30000')
        d.ele('css:button[type=submit]').click()
        closed()
        pick(tab, '.goal button', '存入').click()
        d = dialog()
        d.ele('css:input[inputmode=decimal]').input('5000')
        d.ele('css:button[type=submit]').click()
        closed()
        assert tab.wait.ele_displayed('text:NT$ 5,000', timeout=10)

        # 固定收支與暫停
        pick(tab, '.segmented button', '固定收支', exact=True).click()
        pick(tab, '.month-nav button', '新增固定收支').click()
        d = dialog()
        d.ele('css:input[name=description]').input('測試訂閱')
        d.ele('css:input[name=amount]').input('10')
        # Keep this UI case in the future; due-processing is covered by the DB test.
        d.ele('css:input[name=next_on]').input('2030-01-31', clear=True, by_js=True)
        d.ele('css:button[type=submit]').click()
        closed()
        assert tab.wait.ele_displayed('text:測試訂閱', timeout=10)
        tab.ele('css:button[role=switch][aria-label*="測試訂閱"]').click()
        assert pick(tab, '.badge', '已暫停', exact=True)
        shot(tab, 'budget-recurring-desktop')

        # 投資：建立證券帳戶、期初持倉、手動估值
        nav('investments')
        pick(tab, '.empty button', '新增證券帳戶').click()
        d = dialog()
        d.ele('css:input[name=name]').input('測試證券帳戶')
        d.ele('css:input[name=opening_balance]').input('1000')
        d.ele('css:button[type=submit]').click()
        closed()
        pick(tab, '.page-actions button', '記錄交易').click()
        d = dialog()
        assert d.ele('css:.segmented button.on').text == '期初持倉'
        d.ele('css:select[name=market]').select.by_value('OTHER')
        d.ele('css:input[name=symbol]').input('TEST')
        d.ele('css:input[name=name]').input('測試股票')
        d.ele('css:input[name=quantity]').input('200')
        d.ele('css:input[name=unit_price]').input('10')
        d.ele('css:button[form=trade-form]').click()
        closed()
        assert tab.wait.ele_displayed('css:button.holding', timeout=15)
        assert '測試股票' in tab.ele('css:button.holding').text
        tab.ele('css:button.holding').click()
        pick(dialog(), '.action-list button', '手動估值').click()
        assert tab.wait.ele_displayed('css:input[name=price]', timeout=10)
        d = dialog()
        d.ele('css:input[name=price]').input('12', clear=True)
        d.ele('css:button[type=submit]').click()
        closed()
        assert tab.wait.ele_displayed('text:NT$ 3,400', timeout=10)
        shot(tab, 'investments-desktop')
        mobile(tab, 'investments-mobile')


        # 深色模式與手機版總覽
        nav('overview')
        tab.run_js('localStorage.setItem("mt:theme", JSON.stringify("dark"))')
        tab.refresh()
        assert tab.ele('css:.worth', timeout=15)
        shot(tab, 'dashboard-dark')
        tab.run_js('localStorage.removeItem("mt:theme")')
        tab.refresh()
        assert tab.ele('css:.worth', timeout=15)
        mobile(tab, 'dashboard-mobile')
        tab.run_cdp('Emulation.setDeviceMetricsOverride', width=390, height=844, deviceScaleFactor=1, mobile=True)
        tab.ele('css:.tab-compose').click()
        assert tab.wait.ele_displayed('css:#composer', timeout=10)
        time.sleep(0.6)
        tab.get_screenshot(path=OUTPUT / 'composer-mobile.png')
        tab.run_cdp('Emulation.clearDeviceMetricsOverride')
        print('PASS: visible Edge onboarding/composer, detail edit/split/refund, budget/goal/recurring, holdings/valuation, dark mode, desktop/mobile layout.')
    except Exception:
        if browser:
            tab.get_screenshot(path=OUTPUT / 'failure.png', full_page=True)
            print(tab.ele('tag:body').text.encode('ascii', 'backslashreplace').decode())
        raise
    finally:
        if browser:
            browser.quit()
        result = subprocess.run([str(FIXTURE), identity['id']], capture_output=True, encoding='utf-8')
        if result.returncode:
            raise RuntimeError('UI fixture cleanup failed')


def override_api_origin(tab, api_origin):
    """只改寫 /api/v1/ 請求的 Origin；Next.js 開發資源維持原樣。"""
    def paused(**event):
        headers = [{'name': k, 'value': v} for k, v in event['request']['headers'].items() if k.lower() != 'origin']
        if event['request']['method'] != 'GET':
            headers.append({'name': 'Origin', 'value': api_origin})
        try:
            tab.run_cdp('Fetch.continueRequest', requestId=event['requestId'], headers=headers)
        except Exception:
            pass  # 重新整理時被取消的請求
    tab._driver.set_callback('Fetch.requestPaused', paused)
    tab.run_cdp('Fetch.enable', patterns=[{'urlPattern': '*/api/v1/*', 'requestStage': 'Request'}])


def pick(root, css, text, exact=False, timeout=10):
    end = time.time() + timeout
    while time.time() < end:
        for e in root.eles(f'css:{css}', timeout=0.3):
            t = e.text.strip()
            if (t == text) if exact else (text in t):
                return e
        time.sleep(0.2)
    raise AssertionError(f'{css} containing {text!r} not found')


def shot(tab, name):
    tab.get_screenshot(path=OUTPUT / f'{name}.png', full_page=True)


def mobile(tab, name):
    tab.run_cdp('Emulation.setDeviceMetricsOverride', width=390, height=844, deviceScaleFactor=1, mobile=True)
    assert not tab.run_js('return document.documentElement.scrollWidth > window.innerWidth'), f'{name} horizontal overflow'
    tab.get_screenshot(path=OUTPUT / f'{name}.png', full_page=True)
    tab.run_cdp('Emulation.clearDeviceMetricsOverride')


if __name__ == '__main__':
    main()
