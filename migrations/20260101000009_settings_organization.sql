-- Milestone 5 (BE-07 / BE-08): self-service profile and organization settings.

ALTER TABLE organizations
    ADD COLUMN default_locale  TEXT NOT NULL DEFAULT 'en',
    ADD COLUMN logo_object_key TEXT,
    ADD COLUMN settings        JSONB NOT NULL DEFAULT '{}'::jsonb;

ALTER TABLE organizations
    ADD CONSTRAINT organizations_default_locale_check
    CHECK (default_locale IN ('en', 'fr', 'ar'));

-- Activity timestamp, refreshed at most once per minute by the auth extractor.
ALTER TABLE sessions ADD COLUMN last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now();

-- Rollback
-- ALTER TABLE sessions DROP COLUMN last_seen_at;
-- ALTER TABLE organizations DROP CONSTRAINT organizations_default_locale_check;
-- ALTER TABLE organizations
--     DROP COLUMN settings, DROP COLUMN logo_object_key, DROP COLUMN default_locale;
