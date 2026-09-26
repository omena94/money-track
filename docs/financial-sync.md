# 金融來源（資料來源採集器）

公開版**不附任何銀行、證券、集保或電子發票採集器**。原作者的私人版本曾以各機構網站／App 協定自動登入取數，但這類程式可能違反各機構的服務條款，因此不收錄於公開原始碼。

公開版保留的是與機構無關的部分：

- **同步工作流程**（`apps/api/src/sync.rs`、`source_worker.rs`）：排程、登入次數保護、人工驗證 challenge、取消與逾時。
- **入庫管線**（`source_ingest.rs`）：採集結果先 staging，再以單一交易提交正規化資料；以 coverage 區分完整、部分與未支援區間，避免把「沒觀察到」誤當成「零」。
- **機密保護**（`vault.rs`）：登入設定與 session 狀態依擁有者、連線與用途加密保存。
- **同步 log**（`sync_log.rs`）：事件白名單、雙重遮罩與 SSE 即時推送。

沒有安裝採集器時，`GET /api/v1/connectors` 回傳空清單，介面不會顯示任何可連線的來源；手動帳務、CSV 匯入、預算與資產走勢均可正常使用。

## 自行實作採集器

API 會以子程序執行 `apps/collector/protocol/dist/worker.mjs`（檔案存在時才啟用來源目錄），透過 stdin/stdout 以逐行 JSON 溝通：

| 方向 | `type` / `command` | 用途 |
| --- | --- | --- |
| API → worker | `{"version":1,"command":"sync","connector",...}` | 一次同步工作，含已解密的登入設定與上次狀態 |
| worker → API | `progress` | 回報階段代碼，顯示文字由後端產生 |
| worker → API | `log` | 白名單警告代碼 |
| worker → API | `login_attempt` | 登入前向 API 申請許可（次數保護） |
| worker → API | `waiting_user` | 需要本人輸入驗證碼或選擇，API 轉為介面 challenge |
| worker → API | `session` | 中途保存加密 session 狀態 |
| worker → API | `result` / `failed` | 正規化資料列或錯誤代碼 |
| API → worker | `login_permit` / `continue` / `cancel` | 回覆登入許可、使用者答案或取消 |

自行實作前請確認目標機構的服務條款允許自動化存取，並優先使用官方 API 或使用者自行匯出的檔案。

## 能力與限制

各來源的存款、交易、卡務或持倉覆蓋範圍不同，不能以一個產品成功推論整個來源完整。沒有資料與尚未實作是不同狀態，不應以零取代未知金額。登入失敗受工作次數與連續失敗門檻限制。
