-- Keep related records in the same tenant, including cascades.
ALTER TABLE transactions ADD CONSTRAINT transactions_id_owner_unique UNIQUE(id,owner_id);
ALTER TABLE transactions DROP CONSTRAINT transactions_split_parent_id_fkey;
ALTER TABLE transactions ADD CONSTRAINT transactions_split_parent_owner_fkey
 FOREIGN KEY(split_parent_id,owner_id) REFERENCES transactions(id,owner_id) ON DELETE CASCADE;
ALTER TABLE transactions ADD CONSTRAINT transactions_split_shape
 CHECK(split_parent_id IS NULL OR (NOT is_split_parent AND kind IN ('income','expense')));
ALTER TABLE connections ADD CONSTRAINT connections_id_owner_unique UNIQUE(id,owner_id);
ALTER TABLE sync_jobs DROP CONSTRAINT sync_jobs_connection_id_fkey;
ALTER TABLE sync_jobs ADD CONSTRAINT sync_connection_owner_fkey
 FOREIGN KEY(connection_id,owner_id) REFERENCES connections(id,owner_id);
