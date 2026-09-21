-- 每位使用者自己的分類：分收入／支出／內部資金移動，可自選圖示與顏色。
-- 內建分類（builtin）供自動分類關鍵字使用，只能換圖示、顏色或隱藏，不能改名或刪除。
-- 使用者第一次讀取時由 API 補齊內建分類，並把過去自己輸入過的分類名稱轉為自訂分類。
CREATE TABLE user_categories (
 id uuid PRIMARY KEY,
 owner_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 name text NOT NULL CHECK(length(btrim(name)) BETWEEN 1 AND 30 AND name=btrim(name) AND name<>'未分類'),
 kind text NOT NULL CHECK(kind IN ('expense','income','flow')),
 icon text NOT NULL CHECK(icon ~ '^[a-z0-9-]{1,40}$'),
 color text NOT NULL CHECK(color ~ '^c([1-9]|1[0-2])$'),
 position integer NOT NULL DEFAULT 0,
 hidden boolean NOT NULL DEFAULT false,
 builtin boolean NOT NULL DEFAULT false,
 created_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(owner_id,name)
);
ALTER TABLE user_categories ENABLE ROW LEVEL SECURITY;
ALTER TABLE user_categories FORCE ROW LEVEL SECURITY;
CREATE POLICY owner_only ON user_categories USING(owner_id=current_owner()) WITH CHECK(owner_id=current_owner());
GRANT SELECT,INSERT,UPDATE,DELETE ON user_categories TO money_app;
