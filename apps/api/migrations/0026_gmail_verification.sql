CREATE TABLE gmail_links (
 connection_id uuid PRIMARY KEY REFERENCES connections(id) ON DELETE CASCADE,
 owner_id uuid NOT NULL REFERENCES users(id),
 generation uuid NOT NULL,
 email text NOT NULL,
 tokens_cipher text NOT NULL,
 auto_requested boolean NOT NULL DEFAULT false,
 verified_at timestamptz,
 last_error text,
 updated_at timestamptz NOT NULL DEFAULT now()
);
ALTER TABLE gmail_links ENABLE ROW LEVEL SECURITY;
ALTER TABLE gmail_links FORCE ROW LEVEL SECURITY;
CREATE POLICY owner_only ON gmail_links USING(owner_id=current_owner()) WITH CHECK(owner_id=current_owner());
CREATE TABLE gmail_oauth_attempts (
 state_hash text PRIMARY KEY,
 owner_id uuid NOT NULL REFERENCES users(id),
 connection_id uuid NOT NULL REFERENCES connections(id) ON DELETE CASCADE,
 source_revision integer NOT NULL,
 browser_hash text NOT NULL,
 verifier_cipher text NOT NULL,
 auto_requested boolean NOT NULL DEFAULT false,
 expires_at timestamptz NOT NULL
);
ALTER TABLE gmail_oauth_attempts ENABLE ROW LEVEL SECURITY;
ALTER TABLE gmail_oauth_attempts FORCE ROW LEVEL SECURITY;
CREATE POLICY owner_only ON gmail_oauth_attempts USING(owner_id=current_owner()) WITH CHECK(owner_id=current_owner());
GRANT SELECT,INSERT,UPDATE,DELETE ON gmail_links,gmail_oauth_attempts TO money_app;
