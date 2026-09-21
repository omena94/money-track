-- 訂閱自訂圖示：前端先縮成小圖再上傳，列表直接內嵌回傳，不另開圖片端點。
ALTER TABLE subscriptions
 ADD COLUMN icon_photo bytea CHECK(icon_photo IS NULL OR length(icon_photo)<=65536),
 ADD COLUMN icon_mime text CHECK(icon_mime IS NULL OR icon_mime IN ('image/png','image/jpeg','image/webp')),
 ADD CHECK((icon_photo IS NULL) = (icon_mime IS NULL));
