ALTER TABLE connections ADD COLUMN last_requested_at timestamptz;
CREATE TABLE audit_events (
 id uuid PRIMARY KEY, actor_id uuid NOT NULL REFERENCES users(id), target_id uuid REFERENCES users(id),
 action text NOT NULL, created_at timestamptz NOT NULL DEFAULT now()
);
-- Only the authentication subsystem can manage sync eligibility; no financial grants.
REVOKE ALL ON audit_events FROM PUBLIC;
