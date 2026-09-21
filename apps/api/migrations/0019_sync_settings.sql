-- 同步設定：每位使用者所有來源共用的每日同步時間（台北時間 HH:MM，1 到 6 個），
-- 以及登入保護：單次同步最多嘗試登入幾次、連續失敗幾次就停止自動同步。
CREATE TABLE sync_settings (
 owner_id uuid PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
 times text[] NOT NULL DEFAULT ARRAY['07:00'] CHECK(
  cardinality(times) BETWEEN 1 AND 6
  AND array_to_string(times,',') ~ '^([01][0-9]|2[0-3]):[0-5][0-9](,([01][0-9]|2[0-3]):[0-5][0-9])*$'),
 attempt_limit smallint NOT NULL DEFAULT 1 CHECK(attempt_limit BETWEEN 1 AND 3),
 failure_limit smallint NOT NULL DEFAULT 2 CHECK(failure_limit BETWEEN 1 AND 5),
 updated_at timestamptz NOT NULL DEFAULT now()
);
ALTER TABLE sync_settings ENABLE ROW LEVEL SECURITY;
ALTER TABLE sync_settings FORCE ROW LEVEL SECURITY;
CREATE POLICY owner_only ON sync_settings USING(owner_id=current_owner()) WITH CHECK(owner_id=current_owner());
GRANT SELECT,INSERT,UPDATE,DELETE ON sync_settings TO money_app;

-- 連續失敗上限改為可設定（最多 5），計數欄位放寬
ALTER TABLE connections DROP CONSTRAINT IF EXISTS connections_login_failures_check;
ALTER TABLE connections ADD CONSTRAINT connections_login_failures_check CHECK(login_failures BETWEEN 0 AND 10);

-- 既有每日排程對齊到預設的每天 07:00（台北）
UPDATE connections SET next_run_at=(
  CASE WHEN now() < (date_trunc('day', now() AT TIME ZONE 'Asia/Taipei') + time '07:00') AT TIME ZONE 'Asia/Taipei'
       THEN (date_trunc('day', now() AT TIME ZONE 'Asia/Taipei') + time '07:00') AT TIME ZONE 'Asia/Taipei'
       ELSE (date_trunc('day', now() AT TIME ZONE 'Asia/Taipei') + interval '1 day' + time '07:00') AT TIME ZONE 'Asia/Taipei' END)
WHERE enabled AND daily AND next_run_at IS NOT NULL AND connector<>'hsbc';
