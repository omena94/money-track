CREATE TABLE account_summaries (
 id uuid PRIMARY KEY, owner_id uuid NOT NULL, name text NOT NULL, kind text NOT NULL,
 currency text NOT NULL, institution text, balance numeric(28,8),
 FOREIGN KEY(id,owner_id) REFERENCES accounts(id,owner_id) ON DELETE CASCADE
);
INSERT INTO account_summaries SELECT id,owner_id,name,kind,currency,institution,current_balance+investment_value FROM accounts;
ALTER TABLE account_summaries ENABLE ROW LEVEL SECURITY;
ALTER TABLE account_summaries FORCE ROW LEVEL SECURITY;
CREATE POLICY summary_read ON account_summaries FOR SELECT USING(owner_id=current_owner() OR EXISTS(
 SELECT 1 FROM shares s WHERE s.account_id=account_summaries.id AND s.accepted_by=current_owner() AND s.recipient_email=current_email()));
CREATE POLICY summary_insert ON account_summaries FOR INSERT WITH CHECK(owner_id=current_owner());
CREATE POLICY summary_update ON account_summaries FOR UPDATE USING(owner_id=current_owner()) WITH CHECK(owner_id=current_owner());
CREATE POLICY summary_delete ON account_summaries FOR DELETE USING(owner_id=current_owner());
DROP POLICY account_read ON accounts;
CREATE POLICY account_read ON accounts FOR SELECT USING(owner_id=current_owner());
CREATE FUNCTION update_account_summary() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 INSERT INTO account_summaries(id,owner_id,name,kind,currency,institution,balance)
 VALUES(NEW.id,NEW.owner_id,NEW.name,NEW.kind,NEW.currency,NEW.institution,NEW.current_balance+NEW.investment_value)
 ON CONFLICT(id) DO UPDATE SET name=EXCLUDED.name,kind=EXCLUDED.kind,currency=EXCLUDED.currency,institution=EXCLUDED.institution,balance=EXCLUDED.balance;
 RETURN NULL;
END $$;
CREATE TRIGGER account_summary AFTER INSERT OR UPDATE ON accounts FOR EACH ROW EXECUTE FUNCTION update_account_summary();
