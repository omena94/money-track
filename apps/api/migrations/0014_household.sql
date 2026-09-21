-- 家庭檢視：擁有者授權某個 Email 唯讀查看自己的全部資料，檢視者可合併多人加總。
CREATE TABLE household_viewers (
 id uuid PRIMARY KEY,
 owner_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 viewer_email text NOT NULL CHECK(viewer_email=lower(viewer_email) AND length(viewer_email) BETWEEN 3 AND 254),
 created_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(owner_id,viewer_email)
);
ALTER TABLE household_viewers ENABLE ROW LEVEL SECURITY;
ALTER TABLE household_viewers FORCE ROW LEVEL SECURITY;
CREATE POLICY viewer_read ON household_viewers FOR SELECT USING(owner_id=current_owner() OR viewer_email=current_email());
CREATE POLICY viewer_insert ON household_viewers FOR INSERT WITH CHECK(owner_id=current_owner());
-- 擁有者可撤銷；檢視者也可以自己退出
CREATE POLICY viewer_delete ON household_viewers FOR DELETE USING(owner_id=current_owner() OR viewer_email=current_email());
GRANT SELECT,INSERT,DELETE ON household_viewers TO money_app;
