-- 活動時間獨立於 session，登出或 session 清理後仍保留。
-- 舊活動沒有可靠紀錄，不以加入時間或登入時間推估。
ALTER TABLE users ADD COLUMN last_active_at timestamptz;
