# 貢獻指南

先閱讀 [架構](docs/architecture.md)與 [AGENTS.md](AGENTS.md)。工作開始前更新公開任務，實作完成後記錄變更與實際驗證。研究、真實金融樣本與私人操作資訊留在 Git 排除的 `.local`。

## 可重現檢查

```powershell
npm ci
npm run public:check
npm run typecheck
cargo test -p money-api --lib
npm run build
node --test scripts/tests/check-public.test.mjs
```

整合測試需在獨立測試安裝準備本機 PostgreSQL、Redis 及 runtime 設定後執行 `scripts/Test.ps1`。這個腳本會檢查 API 已停止，並執行含資料庫寫入的測試；不要在日常使用的服務上隨意執行。

UI 驗證位於 `scripts/tests/test_*.py`，需額外安裝 `pip install -r scripts/tests/requirements.txt`、建置 `cargo build -p money-api --example ui_fixture`，在相同 PowerShell 載入 `Import-Environment.ps1`，並啟動測試安裝。使用合成 fixture 和隔離 Edge，輸出只寫入 `.local/ui`。

提交前執行 `npm run public:check -- --staged` 並人工檢阅 staged diff。不要加入機密、真實帳號、個人網址、研究報告、建置產物或 AI 署名。MIT 與第三方聲明必須保留。
