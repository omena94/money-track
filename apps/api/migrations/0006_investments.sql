CREATE TABLE market_quotes (
 market text NOT NULL CHECK(market IN ('TWSE','TPEX')), symbol text NOT NULL, name text NOT NULL,
 price numeric(28,8) NOT NULL CHECK(price>=0), quoted_on date NOT NULL, fetched_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY(market,symbol)
);
CREATE TABLE market_updates(source text PRIMARY KEY, last_attempt timestamptz NOT NULL, last_success timestamptz, rows integer NOT NULL DEFAULT 0, message text NOT NULL DEFAULT '');
CREATE TABLE investment_positions (
 id uuid PRIMARY KEY, owner_id uuid NOT NULL REFERENCES users(id), account_id uuid NOT NULL,
 market text NOT NULL CHECK(market IN ('TWSE','TPEX','OTHER')), symbol text NOT NULL, name text NOT NULL,
 quantity numeric(28,8) NOT NULL DEFAULT 0 CHECK(quantity>=0), cost_basis numeric(28,8) CHECK(cost_basis>=0),
 last_price numeric(28,8) CHECK(last_price>=0), price_date date, price_source text,
 UNIQUE(id,owner_id), UNIQUE(account_id,market,symbol), FOREIGN KEY(account_id,owner_id) REFERENCES accounts(id,owner_id)
);
CREATE TABLE investment_trades (
 id uuid PRIMARY KEY, owner_id uuid NOT NULL REFERENCES users(id), position_id uuid NOT NULL,
 kind text NOT NULL CHECK(kind IN ('opening','buy','sell','dividend','basis')),
 quantity numeric(28,8) NOT NULL, unit_price numeric(28,8), fee numeric(28,8) NOT NULL, tax numeric(28,8) NOT NULL,
 cash_amount numeric(28,8) NOT NULL, realized_gain numeric(28,8), occurred_on date NOT NULL,
 previous_quantity numeric(28,8) NOT NULL, previous_basis numeric(28,8), request_hash text NOT NULL, created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
 UNIQUE(id,owner_id), FOREIGN KEY(position_id,owner_id) REFERENCES investment_positions(id,owner_id)
);
ALTER TABLE transactions ADD COLUMN investment_trade_id uuid;
ALTER TABLE transactions ADD CONSTRAINT investment_transaction_owner FOREIGN KEY(investment_trade_id,owner_id) REFERENCES investment_trades(id,owner_id) ON DELETE CASCADE;
ALTER TABLE accounts ADD COLUMN investment_value numeric(28,8) DEFAULT 0;
ALTER TABLE accounts ADD COLUMN valuation_date date;
ALTER TABLE investment_positions ENABLE ROW LEVEL SECURITY;
ALTER TABLE investment_positions FORCE ROW LEVEL SECURITY;
ALTER TABLE investment_trades ENABLE ROW LEVEL SECURITY;
ALTER TABLE investment_trades FORCE ROW LEVEL SECURITY;
CREATE POLICY position_owner ON investment_positions USING(owner_id=current_owner()) WITH CHECK(owner_id=current_owner());
CREATE POLICY investment_trade_owner ON investment_trades USING(owner_id=current_owner()) WITH CHECK(owner_id=current_owner());
CREATE FUNCTION revalue_positions() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE a uuid; u uuid;
BEGIN
 a:=coalesce(NEW.account_id,OLD.account_id); u:=coalesce(NEW.owner_id,OLD.owner_id);
 UPDATE accounts SET investment_value=(
   SELECT CASE WHEN count(*) FILTER(WHERE quantity>0 AND last_price IS NULL)>0 THEN NULL
   ELSE coalesce(sum(quantity*last_price) FILTER(WHERE quantity>0),0) END
   FROM investment_positions WHERE account_id=a AND owner_id=u),
 valuation_date=(SELECT min(price_date) FROM investment_positions WHERE account_id=a AND owner_id=u AND quantity>0)
 WHERE id=a AND owner_id=u;
 RETURN NULL;
END $$;
CREATE TRIGGER position_valuation AFTER INSERT OR UPDATE OR DELETE ON investment_positions FOR EACH ROW EXECUTE FUNCTION revalue_positions();
