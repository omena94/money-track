ALTER TABLE connections ADD COLUMN manual_only boolean NOT NULL DEFAULT false;

-- Only confirmed second-factor evidence qualifies, not generic login failures.
UPDATE connections c SET manual_only=true, daily=false, next_run_at=NULL
WHERE c.connector='fubon' AND EXISTS (
  SELECT 1 FROM sync_jobs j JOIN sync_job_events e ON e.job_id=j.id AND e.owner_id=j.owner_id
  WHERE j.connection_id=c.id AND j.owner_id=c.owner_id
    AND (e.code='verification_required' OR e.detail LIKE '%DEVICE_CONFIRMATION_REQUIRED%')
);

ALTER TABLE connections ADD CONSTRAINT connections_manual_schedule
  CHECK (NOT manual_only OR (NOT daily AND next_run_at IS NULL));
