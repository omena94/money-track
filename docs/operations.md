# Windows 操作手冊

## 同步即時 log 版本更新（MT-090）

本功能新增 migration `0021_sync_events.sql`（`sync_job_events` 資料表，以及 `sync_jobs.error_code`、`failed_step` 欄位），並需要 Rust API、collector 與 Next.js 配套部署。新 collector 會輸出 `log` 訊息，舊版 API 無法辨識，所以**不可只重建 collector**。依下方更新流程執行：`Backup.ps1`、`Stop.ps1`、`Initialize.ps1`（套用 migration）、`cargo build`、`npm run build`、`Start.ps1 -Production`。停機前先確認沒有執行中或等待驗證的同步工作。過期事件由同步 worker 每小時清理一次。

## 備份角色序列權限（MT-094）

資料表使用 identity／序列時，`pg_dump` 需要備份角色對序列有 SELECT 權限。`Initialize.ps1` 會授權全部序列並設定預設權限；更新後重新執行一次 `Initialize.ps1`，再以 `Backup.ps1` 確認備份成功。

## 設定與安裝

依根目錄 README 安裝工具、依賴並建置。現有腳本使用 PostgreSQL 18 預設安裝路徑；Redis 預設為本機 6379；採集器不需要安裝 Edge／Chrome 或登入互動桌面。請先確認服務可用。尚未提供全新 Windows 的全自動安裝。

`Initialize.ps1` 自動沿用 `.local/postgres-admin.dpapi` 的 PostgreSQL 管理連線設定，建立應用／認證／備份角色並套用 SQL migrations。首次尚未保存時才以隱藏輸入取得密碼，通過唯讀連線檢查後以目前 Windows 使用者的 DPAPI 加密保存。應用角色不得為 superuser 或 BYPASSRLS；備份角色具全資料備份權限，僅供本機維運。現有 `.local/secrets.dpapi` 會沿用，不因開源目錄整理重建。

Google OAuth 需設定自己的用戶端 ID、secret、授權 origin 與回呼。執行 `Set-GoogleOAuth.ps1`，secret 使用隱藏輸入；未傳 `-Origin` 時沿用既有設定，新環境才使用 localhost。回呼為 `APP_ORIGIN/api/v1/auth/google/callback`。

`Import-Environment.ps1` 將 DPAPI 設定載入目前 PowerShell 的程序環境，不會寫成明文設定檔。不要印出環境變數或把機密貼入 issue。

管理連線設定與 runtime 設定分開保存，網站與採集器不會自動載入管理密碼。更新保存的管理登入時執行 `scripts/Set-PostgresAdmin.ps1`，密碼使用隱藏輸入；可用 `-HostName`、`-Port`、`-Username` 指定連線，驗證失敗時保留原加密檔。已提供 `PGPASSWORD` 的目前程序優先沿用其設定；一般情況直接使用已保存憑證，不重複詢問。`Invoke-PostgresAdmin` 僅在管理操作期間設定 libpq 環境變數，成功或失敗都恢復原值。儲存檔損壞或 Windows 使用者不符時顯示錯誤，不覆寫原設定。手動管理指令亦可沿用：

```powershell
. .\scripts\PostgresAdmin.ps1
Invoke-PostgresAdmin -Action {
    & 'C:\Program Files\PostgreSQL\18\bin\psql.exe' -X -w -d postgres -c 'SELECT 1'
}
```

## 日常啟停

```powershell
.\scripts\Start-IfNeeded.ps1
.\scripts\Diagnose.ps1
# 有需要才停止本專案程序：
.\scripts\Stop.ps1
.\scripts\Start.ps1 -Production
```

`Start-IfNeeded.ps1` 沿用健康的既有服務。`Start.ps1` 使用 `target/debug/money-api.exe` 與 Next.js production build，啟動前檢查 3000／3001。網站與 HTTP 採集器均在背景執行。

`Diagnose.ps1` 預設檢查本機；已設定 `APP_ORIGIN` 或傳入 `-Origin` 時另檢查該入口。不會連到原開發者的雲端部署。

## 更新

先確認沒有進行中或等待驗證的同步，建立備份，再安排維護時段：

```powershell
.\scripts\Backup.ps1
.\scripts\Stop.ps1
npm ci
.\.venv\Scripts\python.exe -m pip install -r scripts/requirements.txt
# 有新增 migrations 時執行：
.\scripts\Initialize.ps1
cargo build -p money-api --bin money-api
npm run build
.\scripts\Start.ps1 -Production
.\scripts\Diagnose.ps1
```

SQL migration 順序與檔名應保持不變；移動專案後必須重新編譯 Rust，因為採集器根目錄由編譯時的 workspace 位置解析。

重建 Next.js 前先停止使用該 `.next` 目錄的服務，重建完成再啟動。執行中的程序可能保留舊版資產清單；直接覆寫建置目錄會讓 JavaScript 檔名與磁碟內容不一致，產生 500 或讓畫面停在開啟中。部署後除了 health，應確認實際頁面完成載入與互動。

## 排程與備份

`Install-Startup.ps1` 註冊目前 Windows 使用者登入後啟動及每日 03:00 備份；加上 `-Remove` 移除這兩個排程。僅在需要安裝或變更排程時執行。

`Backup.ps1` 將 PostgreSQL custom archive 與 runtime 設定包成 DPAPI 加密備份，儲存於 `.local/backups`，保留 30 天。`Test-Restore.ps1` 沿用已保存的管理連線設定，在新建的暫存資料庫檢查還原後移除該測試庫；不覆寫既有資料庫。管理登入檔不納入應用備份。DPAPI 備份需要原 Windows 使用者的解密能力，搬機前必須先規劃並驗證復原方式。

備份工具的 DPAPI 程式現在獨立於 `scripts/windows_dpapi.py`，保留既有備份格式。

## 公開 HTTPS

可自行使用 HTTPS 反向代理或 Tunnel 指向 `http://localhost:3000`。不要直接公開 3001、PostgreSQL或 Redis。設定自己的 OAuth origin／callback；隱私頁與聯絡方式應反映自己的部署。

公開專案沒有內建 Cloudflare 帳戶、Zone、Tunnel ID、網域或憑證，也不自動建立雲端資源。既有本機雲端路由不受目錄整理影響。

## MT-096 訂閱版本更新

本功能新增 migration `0022_subscriptions.sql`，需配套建置 Rust API 與 Next.js。依既有更新流程執行 `Backup.ps1`、`Stop.ps1`、`Initialize.ps1`（套用 migration）、`cargo build`、`npm run build` 與啟動。重建會帶上工作目錄中所有尚未部署的改動；多項工作同時進行時，重建前先確認其他改動需要的 migration 已一併套用。

## 成員最近活動更新（MT-099）

先以資料庫管理角色套用 `0023_member_activity.sql`，再啟動新版 Rust API；一般 runtime 角色沿用既有 users UPDATE 權限，不需要擴權。使用既有 `Initialize.ps1` migration 流程時，管理密碼只在本機隱藏輸入。同步部署新版 Next.js；更新期間前端仍相容尚未提供活動欄位的舊 API。

此 migration 只新增 nullable 活動欄位，不修改金融資料或 session，也不推估舊活動。回退程式時可保留該欄位。部署後以既有登入狀態存取頁面，確認管理員清單的「最近活動」更新；切至背景後心跳暫停，切回立即重取。尚未再次使用的成員顯示「尚無活動紀錄」。

驗證指令：匯入本機環境後執行 `cargo test --test member_activity -- --ignored`，該測試使用單一連線的暫存表，不改動真實成員。選用介面回歸測試為 `python scripts/tests/test_member_activity_ui.py`，依賴 `scripts/tests/requirements.txt`，合成資料與隔離瀏覽器產物留在 `.local/member-activity-ui/`。
