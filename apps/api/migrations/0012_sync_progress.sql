-- 同步進度：每個階段的代碼與時間，供網頁即時顯示。
ALTER TABLE sync_jobs ADD COLUMN progress jsonb NOT NULL DEFAULT '[]'::jsonb;
