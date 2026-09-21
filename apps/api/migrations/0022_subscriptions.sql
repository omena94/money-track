-- 訂閱：只記錄「預期會扣什麼」，不產生帳務；實際金額一律來自同步或手動交易。
-- 交易與訂閱的配對在讀取時依比對字串、扣款日與金額計算；這裡只保存使用者的明確決定。
CREATE TABLE subscriptions (
 id uuid PRIMARY KEY,
 owner_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 name text NOT NULL CHECK(length(name) BETWEEN 1 AND 80),
 category text NOT NULL CHECK(length(category) BETWEEN 1 AND 60),
 amount numeric(28,8) NOT NULL CHECK(amount>0),
 currency text NOT NULL CHECK(currency ~ '^[A-Z]{3}$'),
 cycle text NOT NULL CHECK(cycle IN ('monthly','yearly')),
 anchor_day integer NOT NULL CHECK(anchor_day BETWEEN 1 AND 31),
 anchor_month integer CHECK(anchor_month BETWEEN 1 AND 12),
 started_on date NOT NULL,
 ended_on date,
 paused boolean NOT NULL DEFAULT false,
 -- 以 category_text() 正規化（小寫、去空白）的交易摘要片段
 patterns text[] NOT NULL CHECK(cardinality(patterns) BETWEEN 1 AND 10),
 -- 金額允許誤差（百分比）與扣款日前後天數
 tolerance numeric(6,2) NOT NULL DEFAULT 15 CHECK(tolerance BETWEEN 0 AND 100),
 window_days integer NOT NULL DEFAULT 5 CHECK(window_days BETWEEN 0 AND 15),
 -- 代收商（如 Google Payment）：金額不符的交易不自動配對，改列待確認
 require_amount boolean NOT NULL DEFAULT false,
 note text NOT NULL DEFAULT '' CHECK(length(note)<=500),
 created_at timestamptz NOT NULL DEFAULT now(),
 updated_at timestamptz NOT NULL DEFAULT now(),
 CHECK((cycle='yearly') = (anchor_month IS NOT NULL)),
 CHECK(ended_on IS NULL OR ended_on>=started_on),
 UNIQUE(id,owner_id)
);
CREATE INDEX subscriptions_owner ON subscriptions(owner_id);

-- 使用者對單筆交易的確認或排除；自動配對不存。
CREATE TABLE subscription_links (
 id uuid PRIMARY KEY,
 owner_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 subscription_id uuid NOT NULL,
 source_id uuid REFERENCES source_records(id) ON DELETE CASCADE,
 manual_id uuid REFERENCES transactions(id) ON DELETE CASCADE,
 period text NOT NULL CHECK(period ~ '^[0-9]{4}-(0[1-9]|1[0-2])$'),
 state text NOT NULL CHECK(state IN ('confirmed','rejected')),
 created_at timestamptz NOT NULL DEFAULT now(),
 FOREIGN KEY(subscription_id,owner_id) REFERENCES subscriptions(id,owner_id) ON DELETE CASCADE,
 CHECK((source_id IS NULL) <> (manual_id IS NULL))
);
CREATE UNIQUE INDEX subscription_links_source ON subscription_links(subscription_id,source_id) WHERE source_id IS NOT NULL;
CREATE UNIQUE INDEX subscription_links_manual ON subscription_links(subscription_id,manual_id) WHERE manual_id IS NOT NULL;
-- 一筆交易最多確認給一個訂閱
CREATE UNIQUE INDEX subscription_links_source_once ON subscription_links(source_id) WHERE state='confirmed' AND source_id IS NOT NULL;
CREATE UNIQUE INDEX subscription_links_manual_once ON subscription_links(manual_id) WHERE state='confirmed' AND manual_id IS NOT NULL;
CREATE INDEX subscription_links_owner ON subscription_links(owner_id,subscription_id);

-- 「疑似訂閱」中使用者表示不是訂閱的商家
CREATE TABLE subscription_dismissals (
 owner_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 key text NOT NULL CHECK(length(key) BETWEEN 1 AND 120),
 created_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY(owner_id,key)
);

DO $$ DECLARE t text; BEGIN
 FOREACH t IN ARRAY ARRAY['subscriptions','subscription_links','subscription_dismissals'] LOOP
  EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY',t);
  EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY',t);
  EXECUTE format('CREATE POLICY owner_only ON %I USING(owner_id=current_owner()) WITH CHECK(owner_id=current_owner())',t);
 END LOOP;
END $$;
GRANT SELECT,INSERT,UPDATE,DELETE ON subscriptions,subscription_links,subscription_dismissals TO money_app;
