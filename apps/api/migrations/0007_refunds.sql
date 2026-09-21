ALTER TABLE transactions DROP CONSTRAINT transactions_kind_check;
ALTER TABLE transactions ADD CONSTRAINT transactions_kind_check CHECK(kind IN ('income','expense','transfer','adjustment','refund'));
ALTER TABLE transactions ADD COLUMN refund_of uuid;
ALTER TABLE transactions ADD CONSTRAINT refund_owner_link FOREIGN KEY(refund_of,owner_id) REFERENCES transactions(id,owner_id) ON DELETE CASCADE;
ALTER TABLE transactions ADD CONSTRAINT refund_shape CHECK((kind='refund')=(refund_of IS NOT NULL) AND (kind<>'refund' OR amount>0));
