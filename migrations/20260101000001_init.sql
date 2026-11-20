-- Milestone 1: organizations, RBAC, users, invitations, sessions, audit logs.

CREATE TABLE organizations (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT NOT NULL,
    slug        TEXT NOT NULL UNIQUE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Global (system) permissions. Custom per-org roles can be added later.
CREATE TABLE permissions (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    code        TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- organization_id is NULL for system roles shared by every tenant.
CREATE TABLE roles (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID REFERENCES organizations(id) ON DELETE CASCADE,
    code            TEXT NOT NULL UNIQUE,
    name            TEXT NOT NULL,
    description     TEXT,
    is_system       BOOLEAN NOT NULL DEFAULT FALSE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX roles_organization_id_idx ON roles (organization_id);

CREATE TABLE role_permissions (
    role_id       UUID NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    permission_id UUID NOT NULL REFERENCES permissions(id) ON DELETE CASCADE,
    PRIMARY KEY (role_id, permission_id)
);

CREATE TABLE users (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id  UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    first_name       TEXT NOT NULL DEFAULT '',
    last_name        TEXT NOT NULL DEFAULT '',
    email            TEXT NOT NULL,
    phone_number     TEXT,
    avatar_url       TEXT,
    status           TEXT NOT NULL DEFAULT 'invited'
                     CHECK (status IN ('invited', 'active', 'suspended')),
    preferred_locale TEXT NOT NULL DEFAULT 'en',
    password_hash    TEXT,
    auth_provider    TEXT NOT NULL DEFAULT 'local',
    last_login_at    TIMESTAMPTZ,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Emails are stored lower-cased; uniqueness is per tenant.
    CONSTRAINT users_email_lowercase CHECK (email = lower(email)),
    CONSTRAINT users_organization_email_key UNIQUE (organization_id, email)
);

CREATE INDEX users_organization_id_idx ON users (organization_id);

CREATE TABLE user_roles (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role_id         UUID NOT NULL REFERENCES roles(id) ON DELETE RESTRICT,
    assigned_by     UUID REFERENCES users(id) ON DELETE SET NULL,
    assigned_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT user_roles_unique UNIQUE (organization_id, user_id, role_id)
);

CREATE INDEX user_roles_organization_id_idx ON user_roles (organization_id);
CREATE INDEX user_roles_user_id_idx ON user_roles (user_id);

CREATE TABLE invitations (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    email           TEXT NOT NULL,
    phone_number    TEXT,
    role_id         UUID NOT NULL REFERENCES roles(id) ON DELETE RESTRICT,
    token_hash      TEXT NOT NULL UNIQUE,
    invited_by      UUID REFERENCES users(id) ON DELETE SET NULL,
    status          TEXT NOT NULL DEFAULT 'pending'
                    CHECK (status IN ('pending', 'accepted', 'expired', 'revoked')),
    expires_at      TIMESTAMPTZ NOT NULL,
    accepted_at     TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX invitations_organization_id_idx ON invitations (organization_id);
CREATE INDEX invitations_email_idx ON invitations (organization_id, email);

-- At most one live invite per email per tenant.
CREATE UNIQUE INDEX invitations_one_pending_per_email_idx
    ON invitations (organization_id, email)
    WHERE status = 'pending';

CREATE TABLE sessions (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    token_hash      TEXT NOT NULL UNIQUE,
    ip_address      TEXT,
    user_agent      TEXT,
    expires_at      TIMESTAMPTZ NOT NULL,
    revoked_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX sessions_user_id_idx ON sessions (user_id);
CREATE INDEX sessions_organization_id_idx ON sessions (organization_id);

-- Append-only. organization_id is nullable for cross-tenant / pre-auth events.
CREATE TABLE audit_logs (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID REFERENCES organizations(id) ON DELETE SET NULL,
    actor_user_id   UUID REFERENCES users(id) ON DELETE SET NULL,
    action          TEXT NOT NULL,
    entity_type     TEXT NOT NULL,
    entity_id       UUID,
    metadata        JSONB NOT NULL DEFAULT '{}'::jsonb,
    ip_address      TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX audit_logs_organization_id_created_at_idx
    ON audit_logs (organization_id, created_at DESC);
CREATE INDEX audit_logs_action_idx ON audit_logs (action);
