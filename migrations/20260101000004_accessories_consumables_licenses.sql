-- Milestone 3: accessories (bulk + partial returns), consumables (atomic stock
-- ledger + low-stock alerts), software licenses (seat limits + expiry windows).
-- All tables are tenant-scoped like Milestone 2.

-- ------------------------------------------------------------ accessories ----
CREATE TABLE accessories (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    category_id     UUID REFERENCES categories(id) ON DELETE SET NULL,
    manufacturer_id UUID REFERENCES manufacturers(id) ON DELETE SET NULL,
    supplier_id     UUID REFERENCES suppliers(id) ON DELETE SET NULL,
    location_id     UUID REFERENCES locations(id) ON DELETE SET NULL,
    model_number    TEXT,
    total_quantity  INTEGER NOT NULL DEFAULT 0 CHECK (total_quantity >= 0),
    available_quantity INTEGER NOT NULL DEFAULT 0
                        CHECK (available_quantity >= 0 AND available_quantity <= total_quantity),
    min_quantity    INTEGER NOT NULL DEFAULT 0 CHECK (min_quantity >= 0),
    purchase_cost   NUMERIC(14, 2) CHECK (purchase_cost >= 0),
    currency        TEXT NOT NULL DEFAULT 'USD' CHECK (currency ~ '^[A-Z]{3}$'),
    purchase_date   DATE,
    order_number    TEXT,
    notes           TEXT,
    image_url       TEXT,
    is_archived     BOOLEAN NOT NULL DEFAULT FALSE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX accessories_organization_id_idx ON accessories (organization_id);
CREATE INDEX accessories_category_id_idx ON accessories (organization_id, category_id);
CREATE INDEX accessories_archived_idx ON accessories (organization_id, is_archived);

CREATE TABLE accessory_translations (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    accessory_id UUID NOT NULL REFERENCES accessories(id) ON DELETE CASCADE,
    locale       TEXT NOT NULL CHECK (locale IN ('en', 'fr', 'ar')),
    name         TEXT NOT NULL CHECK (btrim(name) <> ''),
    CONSTRAINT accessory_translations_unique UNIQUE (accessory_id, locale)
);

-- Append-only-ish custody rows: rows are updated only to record returns.
CREATE TABLE accessory_assignments (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id      UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    accessory_id         UUID NOT NULL REFERENCES accessories(id) ON DELETE CASCADE,
    assigned_to_user_id  UUID REFERENCES users(id) ON DELETE SET NULL,
    asset_id             UUID REFERENCES assets(id) ON DELETE CASCADE,
    quantity             INTEGER NOT NULL CHECK (quantity > 0),
    returned_quantity    INTEGER NOT NULL DEFAULT 0
                          CHECK (returned_quantity >= 0 AND returned_quantity <= quantity),
    notes                TEXT,
    assigned_by_user_id  UUID REFERENCES users(id) ON DELETE SET NULL,
    returned_by_user_id  UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    returned_at          TIMESTAMPTZ,
    -- A row is held by exactly one of: an employee, or a parent asset.
    CONSTRAINT accessory_assignments_single_target
        CHECK (num_nonnulls(assigned_to_user_id, asset_id) = 1)
);

CREATE INDEX accessory_assignments_accessory_idx
    ON accessory_assignments (organization_id, accessory_id, created_at DESC);
CREATE INDEX accessory_assignments_asset_idx
    ON accessory_assignments (organization_id, asset_id)
    WHERE asset_id IS NOT NULL;
CREATE INDEX accessory_assignments_user_idx
    ON accessory_assignments (organization_id, assigned_to_user_id)
    WHERE assigned_to_user_id IS NOT NULL;

-- ------------------------------------------------------------ consumables ----
CREATE TABLE consumables (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    category_id     UUID REFERENCES categories(id) ON DELETE SET NULL,
    manufacturer_id UUID REFERENCES manufacturers(id) ON DELETE SET NULL,
    supplier_id     UUID REFERENCES suppliers(id) ON DELETE SET NULL,
    location_id     UUID REFERENCES locations(id) ON DELETE SET NULL,
    model_number    TEXT,
    quantity_on_hand INTEGER NOT NULL DEFAULT 0 CHECK (quantity_on_hand >= 0),
    min_stock_alert INTEGER NOT NULL DEFAULT 0 CHECK (min_stock_alert >= 0),
    purchase_cost   NUMERIC(14, 2) CHECK (purchase_cost >= 0),
    currency        TEXT NOT NULL DEFAULT 'USD' CHECK (currency ~ '^[A-Z]{3}$'),
    order_number    TEXT,
    notes           TEXT,
    image_url       TEXT,
    is_archived     BOOLEAN NOT NULL DEFAULT FALSE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX consumables_organization_id_idx ON consumables (organization_id);
CREATE INDEX consumables_archived_idx ON consumables (organization_id, is_archived);
-- Low-stock queries: quantity_on_hand <= min_stock_alert.
CREATE INDEX consumables_low_stock_idx
    ON consumables (organization_id, quantity_on_hand, min_stock_alert);

CREATE TABLE consumable_translations (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    consumable_id UUID NOT NULL REFERENCES consumables(id) ON DELETE CASCADE,
    locale        TEXT NOT NULL CHECK (locale IN ('en', 'fr', 'ar')),
    name          TEXT NOT NULL CHECK (btrim(name) <> ''),
    CONSTRAINT consumable_translations_unique UNIQUE (consumable_id, locale)
);

-- Append-only ledger. `quantity` is the applied delta for adjustments and a
-- positive amount for restock/issue.
CREATE TABLE consumable_transactions (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id     UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    consumable_id       UUID NOT NULL REFERENCES consumables(id) ON DELETE CASCADE,
    transaction_type    TEXT NOT NULL
                        CHECK (transaction_type IN ('restock', 'issue', 'adjustment')),
    quantity            INTEGER NOT NULL CHECK (quantity <> 0),
    user_id             UUID REFERENCES users(id) ON DELETE SET NULL,
    notes               TEXT,
    performed_by_user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT consumable_transactions_positive_except_adjustment
        CHECK (transaction_type = 'adjustment' OR quantity > 0)
);

CREATE INDEX consumable_transactions_consumable_idx
    ON consumable_transactions (organization_id, consumable_id, created_at DESC);
CREATE INDEX consumable_transactions_user_idx
    ON consumable_transactions (organization_id, user_id)
    WHERE user_id IS NOT NULL;

-- ---------------------------------------------------- software licenses ------
CREATE TABLE licenses (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    manufacturer_id UUID REFERENCES manufacturers(id) ON DELETE SET NULL,
    supplier_id     UUID REFERENCES suppliers(id) ON DELETE SET NULL,
    license_type    TEXT NOT NULL DEFAULT 'named'
                    CHECK (license_type IN ('named', 'concurrent', 'subscription', 'perpetual')),
    total_seats     INTEGER NOT NULL DEFAULT 0 CHECK (total_seats >= 0),
    purchase_date   DATE,
    expiration_date DATE,
    purchase_cost   NUMERIC(14, 2) CHECK (purchase_cost >= 0),
    currency        TEXT NOT NULL DEFAULT 'USD' CHECK (currency ~ '^[A-Z]{3}$'),
    order_number    TEXT,
    product_key     TEXT,
    notes           TEXT,
    is_archived     BOOLEAN NOT NULL DEFAULT FALSE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX licenses_organization_id_idx ON licenses (organization_id);
CREATE INDEX licenses_expiration_idx ON licenses (organization_id, expiration_date);
CREATE INDEX licenses_archived_idx ON licenses (organization_id, is_archived);

CREATE TABLE license_translations (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    license_id UUID NOT NULL REFERENCES licenses(id) ON DELETE CASCADE,
    locale     TEXT NOT NULL CHECK (locale IN ('en', 'fr', 'ar')),
    name       TEXT NOT NULL CHECK (btrim(name) <> ''),
    CONSTRAINT license_translations_unique UNIQUE (license_id, locale)
);

CREATE TABLE license_seats (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id      UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    license_id           UUID NOT NULL REFERENCES licenses(id) ON DELETE CASCADE,
    user_id              UUID REFERENCES users(id) ON DELETE SET NULL,
    asset_id             UUID REFERENCES assets(id) ON DELETE CASCADE,
    notes                TEXT,
    assigned_by_user_id  UUID REFERENCES users(id) ON DELETE SET NULL,
    revoked_by_user_id   UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at           TIMESTAMPTZ,
    -- A seat is allocated to exactly one of: an employee, or an asset.
    CONSTRAINT license_seats_single_target
        CHECK (num_nonnulls(user_id, asset_id) = 1)
);

CREATE INDEX license_seats_license_idx
    ON license_seats (organization_id, license_id, created_at DESC);
CREATE INDEX license_seats_asset_idx
    ON license_seats (organization_id, asset_id)
    WHERE asset_id IS NOT NULL AND revoked_at IS NULL;
CREATE INDEX license_seats_user_idx
    ON license_seats (organization_id, user_id)
    WHERE user_id IS NOT NULL AND revoked_at IS NULL;

-- One live seat per target per license.
CREATE UNIQUE INDEX license_seats_active_user_key
    ON license_seats (license_id, user_id)
    WHERE revoked_at IS NULL AND user_id IS NOT NULL;
CREATE UNIQUE INDEX license_seats_active_asset_key
    ON license_seats (license_id, asset_id)
    WHERE revoked_at IS NULL AND asset_id IS NOT NULL;

-- ----------------------------------------------------------- permissions ----
INSERT INTO permissions (code, description) VALUES
    ('accessories.read',   'View accessories and their assignments'),
    ('accessories.write',  'Create, update and delete accessories'),
    ('accessories.assign', 'Assign and return accessories'),
    ('consumables.read',   'View consumables and stock transactions'),
    ('consumables.write',  'Create, update and delete consumables'),
    ('consumables.issue',  'Restock, issue and adjust consumable stock'),
    ('licenses.read',      'View software licenses and seats'),
    ('licenses.write',     'Create, update and delete licenses'),
    ('licenses.assign',    'Allocate and revoke license seats')
ON CONFLICT (code) DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code IN (
    'accessories.read', 'accessories.write', 'accessories.assign',
    'consumables.read', 'consumables.write', 'consumables.issue',
    'licenses.read', 'licenses.write', 'licenses.assign'
)
WHERE r.code = 'admin'
ON CONFLICT DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code IN (
    'accessories.read', 'accessories.write', 'accessories.assign',
    'consumables.read', 'consumables.write', 'consumables.issue',
    'licenses.read', 'licenses.write', 'licenses.assign'
)
WHERE r.code = 'manager'
ON CONFLICT DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code IN (
    'accessories.read', 'accessories.assign',
    'consumables.read', 'consumables.issue',
    'licenses.read', 'licenses.assign'
)
WHERE r.code = 'technician'
ON CONFLICT DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code IN (
    'accessories.read', 'consumables.read', 'licenses.read'
)
WHERE r.code = 'viewer'
ON CONFLICT DO NOTHING;
