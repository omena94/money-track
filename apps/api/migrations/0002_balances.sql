ALTER TABLE accounts ADD COLUMN current_balance numeric(28,8) NOT NULL DEFAULT 0;
UPDATE accounts SET current_balance=opening_balance;
CREATE FUNCTION maintain_balance() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF TG_OP IN ('DELETE','UPDATE') THEN
  UPDATE accounts SET current_balance=current_balance-OLD.amount WHERE id=OLD.account_id AND owner_id=OLD.owner_id;
 END IF;
 IF TG_OP IN ('INSERT','UPDATE') THEN
  UPDATE accounts SET current_balance=current_balance+NEW.amount WHERE id=NEW.account_id AND owner_id=NEW.owner_id;
 END IF;
 RETURN NULL;
END $$;
CREATE TRIGGER transaction_balance AFTER INSERT OR UPDATE OR DELETE ON transactions FOR EACH ROW EXECUTE FUNCTION maintain_balance();
