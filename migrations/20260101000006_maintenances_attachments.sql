-- Milestone 4: maintenance / RMA tickets and polymorphic attachments.

-- --------------------------------------------------------- maintenances -----
-- A ticket moves one asset through repair and puts it back exactly where it
-- was. `previous_status_id` is what the asset had before the ticket opened.
CREATE TABLE asset_maintenances (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id   UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    asset_id          UUID NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    title             TEXT NOT NULL CHECK (btrim(title) <> ''),
    description       TEXT,
    maintenance_type  TEXT NOT NULL DEFAULT 'corrective'
                      CHECK (maintenance_type IN ('preventive', 'corrective', 'inspection')),
    status            TEXT NOT NULL DEFAULT 'open'
                      CHECK (status IN ('open', 'in_progress', 'completed', 'cancelled')),
    priority          TEXT NOT NULL DEFAULT 'medium'
                      CHECK (priority IN ('low', 'medium', 'high', 'critical')),
    supplier_id       UUID REFERENCES suppliers(id) ON DELETE SET NULL,
    rma_reference     TEXT,
    assigned_to_user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    previous_status_id  UUID REFERENCES statuses(id) ON DELETE SET NULL,
    reported_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    scheduled_at      TIMESTAMPTZ,
    started_at        TIMESTAMPTZ,
    completed_at      TIMESTAMPTZ,
    completion_notes  TEXT,
    cost              NUMERIC(14, 2) CHECK (cost >= 0),
    currency          TEXT NOT NULL DEFAULT 'USD' CHECK (currency ~ '^[A-Z]{3}$'),
    created_by_user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX asset_maintenances_org_created_idx
    ON asset_maintenances (organization_id, created_at DESC);
CREATE INDEX asset_maintenances_asset_idx
    ON asset_maintenances (organization_id, asset_id, created_at DESC);
CREATE INDEX asset_maintenances_status_idx
    ON asset_maintenances (organization_id, status, created_at DESC);
CREATE INDEX asset_maintenances_assignee_idx
    ON asset_maintenances (organization_id, assigned_to_user_id)
    WHERE assigned_to_user_id IS NOT NULL;
-- One live ticket per asset.
CREATE UNIQUE INDEX asset_maintenances_one_open_per_asset
    ON asset_maintenances (organization_id, asset_id)
    WHERE status IN ('open', 'in_progress');

-- ---------------------------------------------------------- attachments -----
-- Polymorphic metadata; the bytes live in Garage under `storage_key`.
CREATE TABLE attachments (
    id                 UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id    UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    attachable_type    TEXT NOT NULL
                       CHECK (attachable_type IN ('asset', 'product', 'maintenance',
                                                  'license', 'accessory', 'consumable')),
    attachable_id      UUID NOT NULL,
    category           TEXT NOT NULL DEFAULT 'other'
                       CHECK (category IN ('invoice', 'warranty', 'damage_photo',
                                           'handover_receipt', 'other')),
    file_name          TEXT NOT NULL CHECK (btrim(file_name) <> ''),
    mime_type          TEXT NOT NULL,
    size_bytes         BIGINT NOT NULL CHECK (size_bytes >= 0),
    storage_key        TEXT NOT NULL,
    uploaded_by_user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT attachments_storage_key_key UNIQUE (storage_key)
);

CREATE INDEX attachments_attachable_idx
    ON attachments (organization_id, attachable_type, attachable_id, created_at DESC);
CREATE INDEX attachments_uploader_idx
    ON attachments (organization_id, uploaded_by_user_id)
    WHERE uploaded_by_user_id IS NOT NULL;

-- ----------------------------------------------------------- audit hooks ----
DROP TRIGGER IF EXISTS asset_maintenances_audit ON asset_maintenances;
CREATE TRIGGER asset_maintenances_audit
    AFTER INSERT OR UPDATE OR DELETE ON asset_maintenances
    FOR EACH ROW EXECUTE FUNCTION audit_row_change();
DROP TRIGGER IF EXISTS attachments_audit ON attachments;
CREATE TRIGGER attachments_audit
    AFTER INSERT OR UPDATE OR DELETE ON attachments
    FOR EACH ROW EXECUTE FUNCTION audit_row_change();

-- ----------------------------------------------------------- permissions ----
INSERT INTO permissions (code, description) VALUES
    ('maintenances.read',  'View maintenance and RMA tickets'),
    ('maintenances.write', 'Open, update, complete and cancel maintenance tickets'),
    ('attachments.read',   'View and download attachments'),
    ('attachments.write',  'Upload and delete attachments'),
    ('dashboard.read',     'View the executive dashboard')
ON CONFLICT (code) DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code IN (
    'maintenances.read', 'maintenances.write', 'attachments.read',
    'attachments.write', 'dashboard.read', 'audit.read'
)
WHERE r.code = 'admin'
ON CONFLICT DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code IN (
    'maintenances.read', 'maintenances.write', 'attachments.read',
    'attachments.write', 'dashboard.read', 'audit.read'
)
WHERE r.code = 'manager'
ON CONFLICT DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code IN (
    'maintenances.read', 'maintenances.write', 'attachments.read',
    'attachments.write', 'dashboard.read'
)
WHERE r.code = 'technician'
ON CONFLICT DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code IN ('maintenances.read', 'attachments.read', 'dashboard.read')
WHERE r.code = 'viewer'
ON CONFLICT DO NOTHING;
