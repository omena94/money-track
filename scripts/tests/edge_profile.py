"""自動化 Edge 一律使用獨立設定檔，不沾到使用者平常的 Edge。

Edge 在新設定檔會自動以 Windows 帳號登入並同步擴充功能與頭像；這裡關閉登入、同步與擴充功能。
僅供合成資料的網頁 UI 測試使用，設定檔目錄由測試決定。
"""

ISOLATION_ARGS = (
    "--disable-extensions",
    "--disable-sync",
    "--no-first-run",
    "--no-default-browser-check",
    "--enable-automation",
    "--disable-features=msImplicitSignin",
)
ISOLATION_PREFS = {
    "signin.allowed": False,
    "browser.signin.allowed": False,
    "sync.requested": False,
    # 不儲存密碼
    "credentials_enable_service": False,
}


def isolate(options):
    """套用到 DrissionPage ChromiumOptions，回傳同一個物件。"""
    for arg in ISOLATION_ARGS:
        options.set_argument(arg)
    for key, value in ISOLATION_PREFS.items():
        options.set_pref(key, value)
    return options
