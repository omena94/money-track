ALTER TABLE source_records DROP CONSTRAINT source_records_kind_check;
ALTER TABLE source_records ADD CONSTRAINT source_records_kind_check CHECK(kind IN ('account','transaction','bill','card','holding','investment_transaction','invoice','invoice_item','net_worth'));
