-- Reference accounts keep their source identity; links never rewrite financial data.
CREATE UNIQUE INDEX source_records_id_owner ON source_records(id,owner_id);
CREATE TABLE source_account_links (
 owner_id uuid NOT NULL REFERENCES users(id),
 reference_id uuid PRIMARY KEY,
 primary_source_id uuid,
 primary_manual_id uuid,
 created_at timestamptz NOT NULL DEFAULT now(),
 CHECK (num_nonnulls(primary_source_id,primary_manual_id)=1),
 CHECK (reference_id IS DISTINCT FROM primary_source_id),
 FOREIGN KEY(reference_id,owner_id) REFERENCES source_records(id,owner_id) ON DELETE CASCADE,
 FOREIGN KEY(primary_source_id,owner_id) REFERENCES source_records(id,owner_id) ON DELETE CASCADE,
 FOREIGN KEY(primary_manual_id,owner_id) REFERENCES accounts(id,owner_id) ON DELETE CASCADE
);
CREATE INDEX source_account_links_owner ON source_account_links(owner_id);
ALTER TABLE source_account_links ENABLE ROW LEVEL SECURITY;
ALTER TABLE source_account_links FORCE ROW LEVEL SECURITY;
CREATE POLICY owner_only ON source_account_links USING(owner_id=current_owner()) WITH CHECK(owner_id=current_owner());
GRANT SELECT,INSERT,UPDATE,DELETE ON source_account_links TO money_app;
