ALTER TABLE connections ADD COLUMN login_failures smallint NOT NULL DEFAULT 0 CHECK(login_failures BETWEEN 0 AND 2);
