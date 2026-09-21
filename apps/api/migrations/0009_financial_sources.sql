ALTER TABLE connections ADD COLUMN credentials_cipher text;
ALTER TABLE connections ADD COLUMN state_cipher text;
ALTER TABLE connections ADD COLUMN revision integer NOT NULL DEFAULT 0;
ALTER TABLE connections ADD COLUMN daily boolean NOT NULL DEFAULT false;
ALTER TABLE connections ADD COLUMN next_run_at timestamptz;
ALTER TABLE connections ADD COLUMN last_success_at timestamptz;
ALTER TABLE connections ADD COLUMN updated_at timestamptz NOT NULL DEFAULT now();
ALTER TABLE sync_jobs ADD COLUMN source_revision integer NOT NULL DEFAULT 0;
ALTER TABLE sync_jobs ADD COLUMN challenge jsonb;
ALTER TABLE sync_jobs ADD COLUMN answer_cipher text;
ALTER TABLE sync_jobs ADD COLUMN summary jsonb;
CREATE TABLE source_batches (
 job_id uuid PRIMARY KEY REFERENCES sync_jobs(id) ON DELETE CASCADE,
 owner_id uuid NOT NULL REFERENCES users(id), connection_id uuid NOT NULL,
 cipher text, committed_at timestamptz, created_at timestamptz NOT NULL DEFAULT now(),
 FOREIGN KEY(connection_id,owner_id) REFERENCES connections(id,owner_id)
);
CREATE TABLE source_records (
 id uuid PRIMARY KEY, owner_id uuid NOT NULL REFERENCES users(id), connection_id uuid NOT NULL,
 kind text NOT NULL CHECK(kind IN ('account','transaction','bill','holding','investment_transaction','invoice','invoice_item','net_worth')),
 source_key text NOT NULL CHECK(source_key ~ '^[a-f0-9]{64}$'), account_key text,
 data jsonb NOT NULL CHECK(jsonb_typeof(data)='object'), current boolean NOT NULL DEFAULT true,
 observed_at timestamptz NOT NULL DEFAULT now(), last_job_id uuid REFERENCES sync_jobs(id) ON DELETE SET NULL,
 FOREIGN KEY(connection_id,owner_id) REFERENCES connections(id,owner_id),
 UNIQUE(connection_id,kind,source_key)
);
CREATE INDEX source_records_owner ON source_records(owner_id,connection_id,kind,observed_at DESC);
DO $$ DECLARE t text; BEGIN
 FOREACH t IN ARRAY ARRAY['source_batches','source_records'] LOOP
 EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY',t);
 EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY',t);
 EXECUTE format('CREATE POLICY owner_only ON %I USING(owner_id=current_owner()) WITH CHECK(owner_id=current_owner())',t);
 END LOOP;
END $$;
