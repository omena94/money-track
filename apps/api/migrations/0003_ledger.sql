ALTER TABLE transactions ADD COLUMN split_parent_id uuid REFERENCES transactions(id) ON DELETE CASCADE;
ALTER TABLE transactions ADD COLUMN is_split_parent boolean NOT NULL DEFAULT false;
CREATE FUNCTION maintain_balance_v2() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF TG_OP IN ('DELETE','UPDATE') AND NOT OLD.is_split_parent THEN
  UPDATE accounts SET current_balance=current_balance-OLD.amount WHERE id=OLD.account_id AND owner_id=OLD.owner_id;
 END IF;
 IF TG_OP IN ('INSERT','UPDATE') AND NOT NEW.is_split_parent THEN
  UPDATE accounts SET current_balance=current_balance+NEW.amount WHERE id=NEW.account_id AND owner_id=NEW.owner_id;
 END IF;
 RETURN NULL;
END $$;
DROP TRIGGER transaction_balance ON transactions;
CREATE TRIGGER transaction_balance AFTER INSERT OR UPDATE OR DELETE ON transactions FOR EACH ROW EXECUTE FUNCTION maintain_balance_v2();
CREATE TABLE recurring_entries (
 id uuid PRIMARY KEY, owner_id uuid NOT NULL REFERENCES users(id), account_id uuid NOT NULL,
 amount numeric(28,8) NOT NULL CHECK(amount<>0), kind text NOT NULL CHECK(kind IN ('income','expense')),
 description text NOT NULL, category text NOT NULL, frequency text NOT NULL CHECK(frequency IN ('daily','weekly','monthly','yearly')),
 next_on date NOT NULL, anchor_day integer NOT NULL CHECK(anchor_day BETWEEN 1 AND 31), enabled boolean NOT NULL DEFAULT true,
 FOREIGN KEY(account_id,owner_id) REFERENCES accounts(id,owner_id)
);
ALTER TABLE recurring_entries ENABLE ROW LEVEL SECURITY;
ALTER TABLE recurring_entries FORCE ROW LEVEL SECURITY;
CREATE POLICY recurring_owner ON recurring_entries USING(owner_id=current_owner()) WITH CHECK(owner_id=current_owner());
