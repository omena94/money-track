-- 個人資料：自訂顯示名稱與頭像。Google 登入只更新 name，不會覆蓋 display_name。
-- avatar 為小型設定（類型、表情、顏色、照片版本）；照片本體另存 avatar_photo。
ALTER TABLE users
 ADD COLUMN display_name text CHECK(display_name IS NULL OR (display_name=btrim(display_name) AND length(display_name) BETWEEN 1 AND 40)),
 ADD COLUMN avatar jsonb CHECK(avatar IS NULL OR (jsonb_typeof(avatar)='object' AND length(avatar::text)<=300)),
 ADD COLUMN avatar_photo bytea CHECK(avatar_photo IS NULL OR length(avatar_photo)<=300000),
 ADD COLUMN avatar_mime text CHECK(avatar_mime IS NULL OR avatar_mime IN ('image/png','image/jpeg','image/webp'));
