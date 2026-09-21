ALTER TABLE sync_jobs ADD CONSTRAINT sync_jobs_owner_connection_unique UNIQUE(id,owner_id,connection_id);
ALTER TABLE source_batches DROP CONSTRAINT source_batches_job_id_fkey;
ALTER TABLE source_batches ADD CONSTRAINT source_batches_job_owner_fkey
 FOREIGN KEY(job_id,owner_id,connection_id) REFERENCES sync_jobs(id,owner_id,connection_id) ON DELETE CASCADE;
ALTER TABLE source_records DROP CONSTRAINT source_records_last_job_id_fkey;
ALTER TABLE source_records ADD CONSTRAINT source_records_job_owner_fkey
 FOREIGN KEY(last_job_id,owner_id,connection_id) REFERENCES sync_jobs(id,owner_id,connection_id) ON DELETE SET NULL(last_job_id);
ALTER TABLE source_records ADD CONSTRAINT source_records_account_key_hash CHECK(account_key IS NULL OR account_key ~ '^[a-f0-9]{64}$');
