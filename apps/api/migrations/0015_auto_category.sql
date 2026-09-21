-- 自動分類：同步交易依「使用者規則 → 內建關鍵字」自動帶出分類；使用者也可逐筆修改。
-- 關鍵字比對一律以小寫、去除所有空白（含全形空白）後的文字進行。

-- 內建關鍵字（全體共用、唯讀）。kind 為 NULL 表示收入支出都適用。
CREATE TABLE category_keywords (
 pattern text PRIMARY KEY CHECK(pattern=lower(pattern) AND pattern !~ '[[:space:]　]' AND length(pattern) BETWEEN 1 AND 40),
 category text NOT NULL,
 kind text CHECK(kind IN ('income','expense')),
 priority int NOT NULL DEFAULT 0
);
INSERT INTO category_keywords(pattern,category,kind,priority) VALUES
 -- 收入
 ('薪資','薪資','income',10),('薪轉','薪資','income',10),('薪津','薪資','income',10),('salary','薪資','income',10),('payroll','薪資','income',10),
 ('獎金','獎金','income',10),('年終','獎金','income',10),('bonus','獎金','income',10),
 ('利息','投資收益','income',5),('活存息','投資收益','income',5),('存息','投資收益','income',5),('股利','投資收益','income',5),('股息','投資收益','income',5),('配息','投資收益','income',5),('現金股利','投資收益','income',6),
 ('退稅','其他收入','income',3),('回饋','其他收入','income',2),('現金回饋','其他收入','income',3),
 -- 投資與內部移轉
 ('定期買股','投資',NULL,8),('定期定額','投資',NULL,8),('股票款','投資',NULL,8),('證券','投資',NULL,4),('基金','投資',NULL,4),('申購','投資',NULL,3),('贖回','投資',NULL,3),
 ('綜定轉入','轉帳',NULL,6),('綜存轉','轉帳',NULL,6),('轉定存','轉帳',NULL,6),('存單','投資收益','income',4),
 ('信用卡款','信用卡繳款',NULL,9),('自動扣繳','信用卡繳款',NULL,5),('卡費','信用卡繳款',NULL,6),
 ('無卡提款','提款',NULL,8),('跨行提款','提款',NULL,8),('提款','提款',NULL,6),('atm','提款',NULL,4),
 ('街口','電子支付',NULL,6),('全支付','電子支付',NULL,6),('linepay','電子支付',NULL,6),('悠遊付','電子支付',NULL,6),('icashpay','電子支付',NULL,6),('台灣pay','電子支付',NULL,6),('電支交易','電子支付',NULL,3),
 ('電子轉出','轉帳',NULL,2),('網銀轉帳','轉帳',NULL,2),('跨行轉入','轉帳',NULL,2),('跨行轉帳','轉帳',NULL,2),('匯入匯款','轉帳','income',2),('匯款','轉帳',NULL,1),('轉帳','轉帳',NULL,1),
 -- 交通
 ('etag','交通','expense',6),('停車','交通','expense',6),('加油','交通','expense',6),('中油','交通','expense',6),('台塑石油','交通','expense',6),('高鐵','交通','expense',6),('台鐵','交通','expense',6),('臺鐵','交通','expense',6),('捷運','交通','expense',6),('悠遊卡','交通','expense',5),('一卡通','交通','expense',5),('uber','交通','expense',5),('大車隊','交通','expense',6),('計程車','交通','expense',6),('yoxi','交通','expense',6),('gogoro','交通','expense',6),('ubike','交通','expense',6),('機票','交通','expense',5),('航空','交通','expense',5),('客運','交通','expense',5),
 -- 餐飲
 ('ubereats','餐飲','expense',8),('foodpanda','餐飲','expense',8),('麥當勞','餐飲','expense',7),('肯德基','餐飲','expense',7),('星巴克','餐飲','expense',7),('starbucks','餐飲','expense',7),('路易莎','餐飲','expense',7),('摩斯','餐飲','expense',7),('八方雲集','餐飲','expense',7),('王品','餐飲','expense',7),('瓦城','餐飲','expense',7),('鼎泰豐','餐飲','expense',7),('餐廳','餐飲','expense',5),('餐飲','餐飲','expense',5),('小吃','餐飲','expense',5),('早餐','餐飲','expense',5),('早午餐','餐飲','expense',5),('午餐','餐飲','expense',5),('晚餐','餐飲','expense',5),('便當','餐飲','expense',5),('咖啡','餐飲','expense',5),('飲料','餐飲','expense',5),('7-11','餐飲','expense',4),('統一超商','餐飲','expense',4),('全家','餐飲','expense',4),('萊爾富','餐飲','expense',4),('ok超商','餐飲','expense',4),
 -- 購物
 ('全聯','購物','expense',6),('家樂福','購物','expense',6),('好市多','購物','expense',6),('costco','購物','expense',6),('momo','購物','expense',6),('蝦皮','購物','expense',6),('shopee','購物','expense',6),('pchome','購物','expense',6),('博客來','購物','expense',6),('屈臣氏','購物','expense',6),('康是美','購物','expense',6),('ikea','購物','expense',6),('大創','購物','expense',6),('小北百貨','購物','expense',6),('寶雅','購物','expense',6),('新光三越','購物','expense',6),('sogo','購物','expense',6),('遠東百貨','購物','expense',6),('uniqlo','購物','expense',6),('amazon','購物','expense',5),('淘寶','購物','expense',6),('酷澎','購物','expense',6),('coupang','購物','expense',6),
 -- 居家與帳單
 ('電費','居家','expense',7),('台電','居家','expense',7),('台灣電力','居家','expense',7),('水費','居家','expense',7),('自來水','居家','expense',7),('瓦斯','居家','expense',7),('天然氣','居家','expense',7),('中華電信','居家','expense',7),('遠傳','居家','expense',7),('台灣大哥大','居家','expense',7),('亞太電信','居家','expense',7),('第四台','居家','expense',6),('管理費','居家','expense',7),('房租','居家','expense',7),('租金','居家','expense',6),
 -- 娛樂
 ('netflix','娛樂','expense',7),('spotify','娛樂','expense',7),('youtube','娛樂','expense',6),('disney','娛樂','expense',6),('kkbox','娛樂','expense',7),('steam','娛樂','expense',7),('playstation','娛樂','expense',7),('nintendo','娛樂','expense',7),('威秀','娛樂','expense',7),('秀泰','娛樂','expense',7),('電影','娛樂','expense',5),('apple.com','娛樂','expense',4),('googleplay','娛樂','expense',4),
 -- 醫療、教育、保險、稅費
 ('診所','醫療','expense',7),('醫院','醫療','expense',7),('藥局','醫療','expense',7),('牙醫','醫療','expense',7),('健保','醫療','expense',6),
 ('學費','教育','expense',7),('補習','教育','expense',7),('課程','教育','expense',5),('hahow','教育','expense',7),('udemy','教育','expense',7),
 ('保費','保險','expense',7),('保險','保險','expense',6),('人壽','保險','expense',6),('產險','保險','expense',6),
 ('手續費','手續費','expense',7),('跨行費用','手續費','expense',7),('跨行費','手續費','expense',7),
 ('存款息','投資收益','income',5),('atm存','轉帳',NULL,9),('網銀外存','轉帳',NULL,5),('網銀外匯','轉帳',NULL,5),('卡片轉入','轉帳',NULL,5),
 ('放款繳款','貸款',NULL,8),('房貸','貸款','expense',7),('貸款','貸款','expense',5),
 ('發票中獎','其他收入','income',8),('代發','其他收入','income',2),('年金','其他收入','income',4),
 ('kktix','娛樂','expense',7),('拓元','娛樂','expense',7),('年代售票','娛樂','expense',7),
 ('罰鍰','稅費','expense',7),
 ('國稅','稅費','expense',7),('所得稅','稅費','expense',7),('地價稅','稅費','expense',7),('房屋稅','稅費','expense',7),('牌照稅','稅費','expense',7),('燃料費','稅費','expense',7),('罰單','稅費','expense',6);
GRANT SELECT ON category_keywords TO money_app;

-- 使用者自己的規則：比內建關鍵字優先。
CREATE TABLE category_rules (
 id uuid PRIMARY KEY,
 owner_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 pattern text NOT NULL CHECK(pattern=lower(pattern) AND pattern !~ '[[:space:]　]' AND length(pattern) BETWEEN 1 AND 80),
 category text NOT NULL CHECK(length(category) BETWEEN 1 AND 60),
 created_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(owner_id,pattern)
);
-- 使用者對單筆同步交易的分類、標籤與備註。
CREATE TABLE source_tx_labels (
 record_id uuid PRIMARY KEY REFERENCES source_records(id) ON DELETE CASCADE,
 owner_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 category text NOT NULL CHECK(length(category) BETWEEN 1 AND 240),
 note text NOT NULL DEFAULT '' CHECK(length(note)<=2000),
 tags text[] NOT NULL DEFAULT '{}',
 updated_at timestamptz NOT NULL DEFAULT now()
);
DO $$ DECLARE t text; BEGIN
 FOREACH t IN ARRAY ARRAY['category_rules','source_tx_labels'] LOOP
  EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY',t);
  EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY',t);
  EXECUTE format('CREATE POLICY owner_only ON %I USING(owner_id=current_owner()) WITH CHECK(owner_id=current_owner())',t);
 END LOOP;
END $$;
GRANT SELECT,INSERT,UPDATE,DELETE ON category_rules,source_tx_labels TO money_app;

-- 比對用：小寫並去掉所有空白
CREATE FUNCTION category_text(t text) RETURNS text LANGUAGE sql IMMUTABLE AS
 $$ SELECT regexp_replace(lower(normalize(coalesce(t,''),NFKC)),'[[:space:]　]+','','g') $$;
-- 推測分類：使用者規則（最長關鍵字優先）→ 內建關鍵字。回傳 [分類, 來源]，來源為 rule／auto／none。
-- 以呼叫者身分執行，受 RLS 保護（同步寫入時已在該使用者的 tenant 內）。
CREATE FUNCTION auto_category(owner uuid, t text, kind text) RETURNS text[] LANGUAGE sql STABLE AS $$
 WITH n AS (SELECT category_text(t) AS t)
 SELECT coalesce(
  (SELECT ARRAY[r.category,'rule'] FROM category_rules r,n WHERE r.owner_id=owner AND strpos(n.t,r.pattern)>0 ORDER BY length(r.pattern) DESC,r.created_at DESC LIMIT 1),
  (SELECT ARRAY[k.category,'auto'] FROM category_keywords k,n WHERE strpos(n.t,k.pattern)>0 AND (k.kind IS NULL OR k.kind=auto_category.kind) ORDER BY k.priority DESC,length(k.pattern) DESC LIMIT 1),
  ARRAY[NULL,'none'])
$$;

-- 同步交易寫入時就先分類，讀取收支時不必逐筆比對。
-- 規則變更後把 auto_source 設為 NULL 即會重新計算。
ALTER TABLE source_records ADD COLUMN auto_category text, ADD COLUMN auto_source text CHECK(auto_source IN ('rule','auto','none'));
CREATE FUNCTION source_auto_category() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE c text[];
BEGIN
 IF NEW.kind='transaction' THEN
  c := auto_category(NEW.owner_id, concat_ws(' ',NEW.data->>'description',NEW.data->>'memo'),
   coalesce(NEW.data->>'entry_type',CASE WHEN left(NEW.data->>'amount',1)='-' THEN 'expense' ELSE 'income' END));
  NEW.auto_category := c[1];
  NEW.auto_source := c[2];
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER source_auto_category BEFORE INSERT OR UPDATE OF data,auto_source ON source_records
 FOR EACH ROW EXECUTE FUNCTION source_auto_category();
UPDATE source_records SET auto_source=NULL WHERE kind='transaction';
