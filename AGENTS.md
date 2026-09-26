# Money Track 開發規範

- 使用繁體中文記錄與回覆。
- 架構或 API 改變同步更新 `docs/architecture.md`；部署行為同步更新 `docs/operations.md`。
- 所有新程式、設定、文件與產物均放在本專案根目錄內。
- 全部原生 Windows 執行，不使用 Docker。Next.js 對外 3000，Rust 僅綁定 127.0.0.1:3001。
- 財務資料預設擁有者隔離。跨使用者分享只提供明確授權的唯讀投影。
- 不記錄密碼、cookie、token、完整銀行帳號；機密不放入 Git、命令列參數或瀏覽器公開設定。
- 不在 commit 或 PR 加入 AI 署名。
- 研究、私人操作紀錄及本機驗收產物放在 `.local/`，不得加入 Git；公開文件只保留通用操作、架構及可重現的驗證結果。
- 提交前執行 `npm run public:check` 及 `npm run public:check -- --staged`；保留現有本機機密、資料庫與排程。
- PostgreSQL 管理登入沿用 `.local/postgres-admin.dpapi`（Windows 使用者 DPAPI 加密）；部署／migration 使用 `scripts/Initialize.ps1`，其他管理命令先載入 `scripts/PostgresAdmin.ps1`，在 `Invoke-PostgresAdmin -Action { ... }` 內執行。已有有效保存設定時不再詢問密碼；不得輸出明文或將管理憑證載入一般服務。
