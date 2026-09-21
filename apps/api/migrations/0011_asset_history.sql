-- 同步來源可計入淨資產；每日餘額歷史供資產走勢使用。
ALTER TABLE source_records ADD COLUMN included boolean NOT NULL DEFAULT true;
CREATE TABLE source_balance_history (
 owner_id uuid NOT NULL REFERENCES users(id), connection_id uuid NOT NULL,
 kind text NOT NULL CHECK(kind IN ('account','holding')),
 source_key text NOT NULL CHECK(source_key ~ '^[a-f0-9]{64}$'),
 date date NOT NULL, amount numeric(28,8) NOT NULL,
 currency text NOT NULL CHECK(currency ~ '^[A-Z]{3}$'),
 updated_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY(connection_id,kind,source_key,date),
 FOREIGN KEY(connection_id,owner_id) REFERENCES connections(id,owner_id) ON DELETE CASCADE
);
CREATE INDEX source_balance_history_owner ON source_balance_history(owner_id,date);
-- 手動投資的持倉市值無法由報價回推，逐日記錄。
CREATE TABLE investment_value_snapshots (
 owner_id uuid NOT NULL REFERENCES users(id), account_id uuid NOT NULL,
 date date NOT NULL, value numeric(28,8) NOT NULL,
 PRIMARY KEY(account_id,date),
 FOREIGN KEY(account_id,owner_id) REFERENCES accounts(id,owner_id) ON DELETE CASCADE
);
DO $$ DECLARE t text; BEGIN
 FOREACH t IN ARRAY ARRAY['source_balance_history','investment_value_snapshots'] LOOP
 EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY',t);
 EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY',t);
 EXECUTE format('CREATE POLICY owner_only ON %I USING(owner_id=current_owner()) WITH CHECK(owner_id=current_owner())',t);
 END LOOP;
END $$;
-- 以目前已同步的餘額與市值建立第一個歷史點。
INSERT INTO source_balance_history(owner_id,connection_id,kind,source_key,date,amount,currency)
SELECT owner_id,connection_id,kind,source_key,
 coalesce(left(data->>'as_of',10)::date,(observed_at AT TIME ZONE 'Asia/Taipei')::date),
 (data->>CASE kind WHEN 'account' THEN 'balance' ELSE 'market_value' END)::numeric,
 data->>'currency'
FROM source_records
WHERE kind IN ('account','holding')
 AND data->>CASE kind WHEN 'account' THEN 'balance' ELSE 'market_value' END IS NOT NULL
ON CONFLICT DO NOTHING;
