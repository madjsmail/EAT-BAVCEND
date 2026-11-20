-- Milestone 5: employee directory, email-first auth tokens, offboarding returns.

-- ------------------------------------------------------------------ users --
ALTER TABLE users
    ADD COLUMN employee_id       TEXT,
    ADD COLUMN job_title         TEXT,
    ADD COLUMN department_id     UUID REFERENCES departments(id) ON DELETE SET NULL,
    ADD COLUMN manager_id        UUID REFERENCES users(id) ON DELETE SET NULL,
    ADD COLUMN email_verified_at TIMESTAMPTZ,
    ADD COLUMN deleted_at        TIMESTAMPTZ;

ALTER TABLE users DROP CONSTRAINT users_status_check;
ALTER TABLE users
    ADD CONSTRAINT users_status_check
    CHECK (status IN ('invited', 'active', 'suspended', 'offboarded'));

ALTER TABLE users
    ADD CONSTRAINT users_preferred_locale_check
    CHECK (preferred_locale IN ('en', 'fr', 'ar'));

ALTER TABLE users
    ADD CONSTRAINT users_manager_not_self
    CHECK (manager_id IS NULL OR manager_id <> id);

-- HR id is unique per tenant, case-insensitively, ignoring blanks.
CREATE UNIQUE INDEX users_employee_id_key
    ON users (organization_id, lower(employee_id))
    WHERE employee_id IS NOT NULL AND btrim(employee_id) <> '';

CREATE INDEX users_department_id_idx ON users (organization_id, department_id);
CREATE INDEX users_manager_id_idx ON users (organization_id, manager_id);
CREATE INDEX users_status_idx ON users (organization_id, status) WHERE deleted_at IS NULL;

-- ------------------------------------------------------------- auth tokens --
-- Single-use, hashed tokens for invitations and magic logins. Never store raw.
CREATE TABLE auth_tokens (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash      TEXT NOT NULL UNIQUE,
    purpose         TEXT NOT NULL CHECK (purpose IN ('invite', 'login')),
    expires_at      TIMESTAMPTZ NOT NULL,
    consumed_at     TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX auth_tokens_user_idx ON auth_tokens (organization_id, user_id);
-- At most one live token per user/purpose; issuing a new one supersedes it.
CREATE UNIQUE INDEX auth_tokens_one_live_idx
    ON auth_tokens (organization_id, user_id, purpose)
    WHERE consumed_at IS NULL;

-- ---------------------------------------------------------- return requests --
CREATE TABLE return_requests (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    entity_type     TEXT NOT NULL CHECK (entity_type IN ('asset', 'accessory', 'license')),
    entity_id       UUID NOT NULL,
    -- Accessories may have a partial outstanding quantity; assets/licenses are 1.
    quantity        INTEGER CHECK (quantity IS NULL OR quantity > 0),
    status          TEXT NOT NULL DEFAULT 'pending'
                    CHECK (status IN ('pending', 'returned', 'cancelled')),
    requested_by    UUID REFERENCES users(id) ON DELETE SET NULL,
    resolved_by     UUID REFERENCES users(id) ON DELETE SET NULL,
    resolved_at     TIMESTAMPTZ,
    notes           TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX return_requests_user_idx
    ON return_requests (organization_id, user_id, status);
CREATE UNIQUE INDEX return_requests_one_pending_idx
    ON return_requests (organization_id, entity_type, entity_id)
    WHERE status = 'pending';

-- ------------------------------------------------------- rbac: employees --
INSERT INTO permissions (code, description) VALUES
    ('users.create',   'Create employees'),
    ('users.delete',   'Soft-delete employees'),
    ('users.import',   'Bulk import employees'),
    ('users.offboard', 'Offboard employees and open return requests')
ON CONFLICT (code) DO NOTHING;

-- Admin keeps full control.
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code IN (
    'users.create', 'users.delete', 'users.import', 'users.offboard'
)
WHERE r.code = 'admin'
ON CONFLICT DO NOTHING;

-- Managers run the people directory; deleting stays admin-only.
INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code IN (
    'users.create', 'users.import', 'users.offboard'
)
WHERE r.code = 'manager'
ON CONFLICT DO NOTHING;

-- Base role for regular employees (imported users get it by default).
INSERT INTO roles (code, name, description, is_system)
VALUES ('employee', 'Employee', 'Base role for regular employees', TRUE)
ON CONFLICT (code) DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code IN ('organizations.read', 'users.read')
WHERE r.code = 'employee'
ON CONFLICT DO NOTHING;
