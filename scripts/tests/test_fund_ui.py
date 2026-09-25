"""Visible Edge acceptance for fund asset details, disposable data only."""
import json,os,socket,subprocess
from urllib.parse import urlparse
from test_sources_ui import ROOT,FIXTURE,OUTPUT,isolate,pick
from DrissionPage import Chromium,ChromiumOptions
def main():
    identity=json.loads(subprocess.run([str(FIXTURE)],env={**os.environ,'MONEY_UI_SOURCES':'1','MONEY_UI_FUNDS':'1'},capture_output=True,check=True,encoding='utf-8').stdout)
    browser=None
    try:
        with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
        options=ChromiumOptions(read_file=False).set_browser_path(r'C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe').set_user_data_path(str(ROOT/'.local/fund-ui-profile')).set_local_port(port).headless(False).set_argument('--window-size','1440,1050')
        isolate(options);browser=Chromium(options);tab=browser.new_tab();origin=os.environ.get('APP_ORIGIN','http://localhost:3000');tab.get(origin)
        tab.run_cdp('Network.setCookie',name='money_session',value=identity['session'],domain=urlparse(origin).hostname,path='/',httpOnly=True,secure=origin.startswith('https'),sameSite='Lax')
        tab.refresh();tab.ele('css:.sidebar a.nav-item[href="#/assets"]',timeout=15).click()
        pick(tab,'.inst','集保 e 存摺').click();entry=pick(tab,'.ledger-detail .row','測試基金');assert '100 單位' in entry.text;entry.click()
        sheet=tab.eles('css:[role="dialog"]')[-1];assert '投資本金' in sheet.text and '未實現損益' in sheet.text and '累積配息' in sheet.text and '200,000' in sheet.text
        tab.get_screenshot(path=OUTPUT/'fund-detail-desktop.png')
        tab.run_cdp('Emulation.setDeviceMetricsOverride',width=390,height=844,deviceScaleFactor=1,mobile=True)
        assert tab.run_js('return document.documentElement.scrollWidth <= window.innerWidth')
        tab.get_screenshot(path=OUTPUT/'fund-detail-mobile.png')
        print('Fund UI passed: units, cost, gain, NAV date, dividends, desktop/mobile; disposable fixture only.')
    finally:
        if browser:browser.quit()
        subprocess.run([str(FIXTURE),identity['id']],capture_output=True,check=True)
if __name__=='__main__':main()
