-- 同步事件紀錄：每個工作的逐行 log，供網頁即時顯示與事後診斷。
-- 文字由後端依白名單代碼產生；detail 只存已遮罩、截斷的錯誤摘要，不存帳密、cookie 或完整帳號。
ALTER TABLE sync_jobs ADD COLUMN error_code text CHECK(error_code IS NULL OR error_code ~ '^[a-z_]{1,40}$');
ALTER TABLE sync_jobs ADD COLUMN failed_step text CHECK(failed_step IS NULL OR failed_step ~ '^[a-z_]{1,40}$');
CREATE TABLE sync_job_events (
 id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
 job_id uuid NOT NULL REFERENCES sync_jobs(id) ON DELETE CASCADE,
 owner_id uuid NOT NULL REFERENCES users(id),
 level text NOT NULL CHECK(level IN ('info','warn','error')),
 step text CHECK(step IS NULL OR step ~ '^[a-z_]{1,40}$'),
 code text NOT NULL CHECK(code ~ '^[a-z_]{1,40}$'),
 message text NOT NULL CHECK(length(message) BETWEEN 1 AND 300),
 detail text CHECK(detail IS NULL OR length(detail) <= 300),
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX sync_job_events_job ON sync_job_events(job_id, id);
CREATE INDEX sync_job_events_age ON sync_job_events(created_at);
ALTER TABLE sync_job_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE sync_job_events FORCE ROW LEVEL SECURITY;
CREATE POLICY owner_only ON sync_job_events USING(owner_id=current_owner()) WITH CHECK(owner_id=current_owner());
GRANT SELECT,INSERT,DELETE ON sync_job_events TO money_app;
