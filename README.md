# Money Track

可自行架設的個人與家庭理財工作台，使用 Next.js、Rust/Axum、PostgreSQL 與 Redis，原生 Windows 執行。

> 原作者：Alan (alanting9999)

支援手動帳務、CSV 匯入匯出、預算、固定收支、訂閱追蹤、資產走勢、分類、家庭唯讀分享，以及可插拔的資料來源同步流程（排程、本人驗證 challenge、加密狀態、staging 入庫與即時 log）。

**公開版不附銀行、證券、集保或電子發票採集器**：這類自動登入程式可能違反各機構服務條款，因此只保留與機構無關的同步框架與協定，詳見[金融來源](docs/financial-sync.md)。未安裝採集器時介面不顯示任何可連線來源，其餘功能照常使用。

## 專案目錄

```text
apps/
  web/                    Next.js 介面與前端測試
  api/                    Rust API、SQL migrations、整合測試
docs/                     通用架構、安裝操作與開發紀錄
scripts/                  建置、啟停、備份及發布前檢查
  tests/                  本機 UI 與傳輸驗證
.local/                   私人設定、研究、日誌、備份（Git 排除）
```

依賴與建置產物不納入版本控制。公開建置不需要相鄰的參考專案。

## 安裝

需要 Windows、Git、Node.js 22 以上、Rust MSVC 工具鏈、Python 3.11、PostgreSQL 18、Redis。現有管理腳本預設 PostgreSQL 安裝於 `C:\Program Files\PostgreSQL\18\bin`；

在專案根目錄使用 PowerShell：

```powershell
npm ci
python -m venv .venv
.\.venv\Scripts\python.exe -m pip install -r scripts/requirements.txt
.\scripts\Initialize.ps1 -AdminEmail 'you@example.com'
.\scripts\Set-GoogleOAuth.ps1 -Origin 'http://localhost:3000'
cargo build -p money-api --bin money-api
npm run build
.\scripts\Start.ps1 -Production
.\scripts\Diagnose.ps1
```

先啟動本機 PostgreSQL 與 Redis；在自己的 Google OAuth 專案設定 Web 用戶端與回呼 `http://localhost:3000/api/v1/auth/google/callback`。帳密由腳本以隱藏輸入收取，設定保存在 `.local/secrets.dpapi`。本專案不附帶帳戶、機密或已設定的雲端資源。

瀏覽 `http://localhost:3000`。Rust 僅監聽 `127.0.0.1:3001`，由 Next.js 代理。對外部署需自行設定 HTTPS 與對應 OAuth origin／callback，詳見[操作手冊](docs/operations.md)。

## 開發與檢查

```powershell
npm run public:check
npm run typecheck
npx vitest run apps/web/tests
cargo test -p money-api --lib
```

資料庫與 UI 整合測試需另外準備測試環境，詳見[貢獻指南](CONTRIBUTING.md)。[架構文件](docs/architecture.md)說明資料隔離與採集流程；[金融來源說明](docs/financial-sync.md)記錄能力與限制。

## 授權與隱私

採用 [MIT License](LICENSE)，第三方程式保留[原始授權聲明](THIRD_PARTY_NOTICES.md)。銀行官方圖像不包含在公開原始碼中；未安裝圖像時介面會顯示通用圖示。

研究筆記、實帳驗收、銀行回應、畫面截圖、DPAPI 檔案及備份均不應提交。請勿直接壓縮整個工作目錄分享；提交前依[發布檢查](docs/publication.md)確認實際 Git 內容。
