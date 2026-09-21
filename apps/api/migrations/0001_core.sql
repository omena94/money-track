CREATE TABLE users (
 id uuid PRIMARY KEY, google_sub text NOT NULL UNIQUE, email text NOT NULL, name text NOT NULL,
 is_admin boolean NOT NULL DEFAULT false, sync_approved boolean NOT NULL DEFAULT false,
 created_at timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX users_email ON users(lower(email));
CREATE TABLE sessions (
 token_hash text PRIMARY KEY, user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 csrf_token text NOT NULL, expires_at timestamptz NOT NULL, created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE oauth_attempts (
 state_hash text PRIMARY KEY, browser_hash text NOT NULL, verifier text NOT NULL,
 nonce text NOT NULL, expires_at timestamptz NOT NULL
);
CREATE FUNCTION current_owner() RETURNS uuid LANGUAGE sql STABLE AS
 $$ SELECT nullif(current_setting('app.user_id', true),'')::uuid $$;
CREATE FUNCTION current_email() RETURNS text LANGUAGE sql STABLE AS
 $$ SELECT lower(nullif(current_setting('app.email', true),'')) $$;

CREATE TABLE accounts (
 id uuid PRIMARY KEY, owner_id uuid NOT NULL REFERENCES users(id), name text NOT NULL CHECK(length(name) BETWEEN 1 AND 120),
 kind text NOT NULL CHECK(kind IN ('cash','bank','credit','investment','property','insurance','loan','other')),
 currency text NOT NULL CHECK(currency ~ '^[A-Z]{3}$'), opening_balance numeric(28,8) NOT NULL DEFAULT 0,
 institution text, created_at timestamptz NOT NULL DEFAULT now(), UNIQUE(id, owner_id)
);
CREATE TABLE transactions (
 id uuid PRIMARY KEY, owner_id uuid NOT NULL REFERENCES users(id), account_id uuid NOT NULL,
 amount numeric(28,8) NOT NULL CHECK(amount <> 0), kind text NOT NULL CHECK(kind IN ('income','expense','transfer','adjustment')),
 occurred_on date NOT NULL, description text NOT NULL CHECK(length(description) BETWEEN 1 AND 500),
 category text NOT NULL DEFAULT '未分類', tags text[] NOT NULL DEFAULT '{}', note text NOT NULL DEFAULT '',
 transfer_id uuid, import_key text, created_at timestamptz NOT NULL DEFAULT now(),
 FOREIGN KEY(account_id,owner_id) REFERENCES accounts(id,owner_id),
 UNIQUE(owner_id,import_key), CHECK((kind='transfer') = (transfer_id IS NOT NULL)),
 CHECK(kind NOT IN ('income','expense') OR (kind='income' AND amount>0) OR (kind='expense' AND amount<0))
);
CREATE INDEX transactions_account_date ON transactions(owner_id,account_id,occurred_on DESC);
CREATE TABLE shares (
 id uuid PRIMARY KEY, owner_id uuid NOT NULL REFERENCES users(id), account_id uuid NOT NULL,
 recipient_email text NOT NULL CHECK(recipient_email=lower(recipient_email)),
 permission text NOT NULL CHECK(permission IN ('balance','details')),
 accepted_by uuid REFERENCES users(id), accepted_at timestamptz, created_at timestamptz NOT NULL DEFAULT now(),
 FOREIGN KEY(account_id,owner_id) REFERENCES accounts(id,owner_id), UNIQUE(account_id,recipient_email)
);
CREATE TABLE budgets (
 id uuid PRIMARY KEY, owner_id uuid NOT NULL REFERENCES users(id), month text NOT NULL CHECK(month ~ '^[0-9]{4}-(0[1-9]|1[0-2])$'),
 category text NOT NULL, amount numeric(28,8) NOT NULL CHECK(amount>0), currency text NOT NULL CHECK(currency ~ '^[A-Z]{3}$'),
 UNIQUE(owner_id,month,category,currency)
);
CREATE TABLE goals (
 id uuid PRIMARY KEY, owner_id uuid NOT NULL REFERENCES users(id), name text NOT NULL,
 target numeric(28,8) NOT NULL CHECK(target>0), saved numeric(28,8) NOT NULL DEFAULT 0 CHECK(saved>=0),
 currency text NOT NULL CHECK(currency ~ '^[A-Z]{3}$'), due_on date
);
CREATE TABLE connections (
 id uuid PRIMARY KEY, owner_id uuid NOT NULL REFERENCES users(id), connector text NOT NULL,
 enabled boolean NOT NULL DEFAULT false, created_at timestamptz NOT NULL DEFAULT now(), UNIQUE(owner_id,connector)
);
CREATE TABLE sync_jobs (
 id uuid PRIMARY KEY, owner_id uuid NOT NULL REFERENCES users(id), connection_id uuid NOT NULL REFERENCES connections(id),
 status text NOT NULL CHECK(status IN ('queued','running','waiting_user','succeeded','partial','failed','cancelled')),
 message text NOT NULL DEFAULT '', created_at timestamptz NOT NULL DEFAULT now(), updated_at timestamptz NOT NULL DEFAULT now(),
 lease_until timestamptz, attempts integer NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX active_sync ON sync_jobs(connection_id) WHERE status IN ('queued','running','waiting_user');
CREATE TABLE notifications (
 id uuid PRIMARY KEY, owner_id uuid NOT NULL REFERENCES users(id), title text NOT NULL, body text NOT NULL,
 read_at timestamptz, created_at timestamptz NOT NULL DEFAULT now()
);

ALTER TABLE accounts ENABLE ROW LEVEL SECURITY;
ALTER TABLE accounts FORCE ROW LEVEL SECURITY;
ALTER TABLE transactions ENABLE ROW LEVEL SECURITY;
ALTER TABLE transactions FORCE ROW LEVEL SECURITY;
ALTER TABLE shares ENABLE ROW LEVEL SECURITY;
ALTER TABLE shares FORCE ROW LEVEL SECURITY;
CREATE POLICY share_read ON shares FOR SELECT USING(owner_id=current_owner() OR recipient_email=current_email());
CREATE POLICY share_insert ON shares FOR INSERT WITH CHECK(owner_id=current_owner());
CREATE POLICY share_delete ON shares FOR DELETE USING(owner_id=current_owner());
-- Recipient accepts through a restricted SECURITY DEFINER function; it cannot rewrite an invitation.
CREATE POLICY share_update ON shares FOR UPDATE USING(owner_id=current_owner()) WITH CHECK(owner_id=current_owner());
CREATE POLICY account_read ON accounts FOR SELECT USING(owner_id=current_owner() OR EXISTS(
 SELECT 1 FROM shares s WHERE s.account_id=accounts.id AND s.accepted_by=current_owner() AND s.recipient_email=current_email()));
CREATE POLICY account_insert ON accounts FOR INSERT WITH CHECK(owner_id=current_owner());
CREATE POLICY account_update ON accounts FOR UPDATE USING(owner_id=current_owner()) WITH CHECK(owner_id=current_owner());
CREATE POLICY account_delete ON accounts FOR DELETE USING(owner_id=current_owner());
CREATE POLICY transaction_read ON transactions FOR SELECT USING(owner_id=current_owner() OR EXISTS(
 SELECT 1 FROM shares s WHERE s.account_id=transactions.account_id AND s.accepted_by=current_owner() AND s.recipient_email=current_email() AND s.permission='details'));
CREATE POLICY transaction_insert ON transactions FOR INSERT WITH CHECK(owner_id=current_owner());
CREATE POLICY transaction_update ON transactions FOR UPDATE USING(owner_id=current_owner()) WITH CHECK(owner_id=current_owner());
CREATE POLICY transaction_delete ON transactions FOR DELETE USING(owner_id=current_owner());
DO $$ DECLARE t text; BEGIN
 FOREACH t IN ARRAY ARRAY['budgets','goals','connections','sync_jobs','notifications'] LOOP
 EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY',t);
 EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY',t);
 EXECUTE format('CREATE POLICY owner_only ON %I USING(owner_id=current_owner()) WITH CHECK(owner_id=current_owner())',t);
 END LOOP;
END $$;
