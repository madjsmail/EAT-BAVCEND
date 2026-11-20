-- Milestone 2: localized lookups (translation tables), product catalog,
-- universal asset registry, custody history.
--
-- Every lookup is tenant-scoped: `organization_id` is mandatory and all reads
-- filter on it. Localized names live in `<entity>_translations`, never in JSON.

-- ------------------------------------------------------------------ codes ----
-- `code` feeds the asset tag (COMM.HDST.TLPH.0088). Uppercase, 1-8 chars.

-- ----------------------------------------------------------- departments ----
CREATE TABLE departments (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    code            TEXT NOT NULL CHECK (code ~ '^[A-Z0-9]{1,8}$'),
    email           TEXT,
    phone_number    TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT departments_org_code_key UNIQUE (organization_id, code)
);

CREATE INDEX departments_organization_id_idx ON departments (organization_id);

CREATE TABLE department_translations (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    department_id UUID NOT NULL REFERENCES departments(id) ON DELETE CASCADE,
    locale        TEXT NOT NULL CHECK (locale IN ('en', 'fr', 'ar')),
    name          TEXT NOT NULL CHECK (btrim(name) <> ''),
    CONSTRAINT department_translations_unique UNIQUE (department_id, locale)
);

-- ------------------------------------------------------------ categories ----
CREATE TABLE categories (
    id                                 UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id                    UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    code                               TEXT NOT NULL CHECK (code ~ '^[A-Z0-9]{1,8}$'),
    category_type                      TEXT NOT NULL
                                       CHECK (category_type IN ('accessory', 'asset', 'consumable', 'component')),
    skip_checkout_confirmation_emails  BOOLEAN NOT NULL DEFAULT FALSE,
    icon_url                           TEXT,
    created_at                         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at                         TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT categories_org_code_key UNIQUE (organization_id, code)
);

CREATE INDEX categories_organization_id_idx ON categories (organization_id);

CREATE TABLE category_translations (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    category_id UUID NOT NULL REFERENCES categories(id) ON DELETE CASCADE,
    locale      TEXT NOT NULL CHECK (locale IN ('en', 'fr', 'ar')),
    name        TEXT NOT NULL CHECK (btrim(name) <> ''),
    CONSTRAINT category_translations_unique UNIQUE (category_id, locale)
);

-- --------------------------------------------------------- manufacturers ----
CREATE TABLE manufacturers (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    url             TEXT,
    support_url     TEXT,
    support_phone   TEXT,
    support_email   TEXT,
    notes           TEXT,
    logo_url        TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX manufacturers_organization_id_idx ON manufacturers (organization_id);

CREATE TABLE manufacturer_translations (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    manufacturer_id UUID NOT NULL REFERENCES manufacturers(id) ON DELETE CASCADE,
    locale          TEXT NOT NULL CHECK (locale IN ('en', 'fr', 'ar')),
    name            TEXT NOT NULL CHECK (btrim(name) <> ''),
    CONSTRAINT manufacturer_translations_unique UNIQUE (manufacturer_id, locale)
);

-- --------------------------------------------------------------- statuses ----
-- `code` is the stable machine key ('deployed', 'ready_to_deploy', ...);
-- custody transitions resolve statuses by code and fall back to `status_type`.
CREATE TABLE statuses (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    code            TEXT NOT NULL CHECK (code ~ '^[a-z0-9_]{1,32}$'),
    status_type     TEXT NOT NULL
                    CHECK (status_type IN ('deployable', 'deployed', 'undeployable', 'pending', 'archived')),
    color_code      TEXT NOT NULL DEFAULT '#6B7280' CHECK (color_code ~ '^#[0-9A-Fa-f]{6}$'),
    show_in_nav     BOOLEAN NOT NULL DEFAULT FALSE,
    notes           TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT statuses_org_code_key UNIQUE (organization_id, code)
);

CREATE INDEX statuses_organization_id_idx ON statuses (organization_id);

CREATE TABLE status_translations (
    id        UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    status_id UUID NOT NULL REFERENCES statuses(id) ON DELETE CASCADE,
    locale    TEXT NOT NULL CHECK (locale IN ('en', 'fr', 'ar')),
    name      TEXT NOT NULL CHECK (btrim(name) <> ''),
    CONSTRAINT status_translations_unique UNIQUE (status_id, locale)
);

-- -------------------------------------------------------------- locations ----
CREATE TABLE locations (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    code            TEXT NOT NULL CHECK (code ~ '^[A-Z0-9-]{1,16}$'),
    parent_id       UUID REFERENCES locations(id) ON DELETE RESTRICT,
    location_type   TEXT NOT NULL DEFAULT 'site'
                    CHECK (location_type IN ('site', 'building', 'floor', 'room', 'shelf')),
    address_line1   TEXT,
    address_line2   TEXT,
    city            TEXT,
    state_region    TEXT,
    postal_code     TEXT,
    country_code    TEXT CHECK (country_code IS NULL OR country_code ~ '^[A-Z]{2}$'),
    latitude        DOUBLE PRECISION CHECK (latitude IS NULL OR latitude BETWEEN -90 AND 90),
    longitude       DOUBLE PRECISION CHECK (longitude IS NULL OR longitude BETWEEN -180 AND 180),
    phone_number    TEXT,
    email           TEXT,
    manager_user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    notes           TEXT,
    is_active       BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT locations_org_code_key UNIQUE (organization_id, code),
    -- A location cannot be its own parent (deeper cycles are rejected in app code).
    CONSTRAINT locations_parent_not_self CHECK (parent_id IS NULL OR parent_id <> id)
);

CREATE INDEX locations_organization_id_idx ON locations (organization_id);
CREATE INDEX locations_parent_id_idx ON locations (parent_id);

CREATE TABLE location_translations (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    location_id UUID NOT NULL REFERENCES locations(id) ON DELETE CASCADE,
    locale      TEXT NOT NULL CHECK (locale IN ('en', 'fr', 'ar')),
    name        TEXT NOT NULL CHECK (btrim(name) <> ''),
    CONSTRAINT location_translations_unique UNIQUE (location_id, locale)
);

-- -------------------------------------------------------------- suppliers ----
CREATE TABLE suppliers (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    contact_name    TEXT,
    email           TEXT,
    phone_number    TEXT,
    address         TEXT,
    url             TEXT,
    notes           TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX suppliers_organization_id_idx ON suppliers (organization_id);

CREATE TABLE supplier_translations (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    supplier_id UUID NOT NULL REFERENCES suppliers(id) ON DELETE CASCADE,
    locale      TEXT NOT NULL CHECK (locale IN ('en', 'fr', 'ar')),
    name        TEXT NOT NULL CHECK (btrim(name) <> ''),
    CONSTRAINT supplier_translations_unique UNIQUE (supplier_id, locale)
);

-- -------------------------------------------------- depreciation models ------
CREATE TABLE depreciation_models (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    months          INTEGER NOT NULL CHECK (months > 0),
    floor_value     NUMERIC(14, 2) NOT NULL DEFAULT 0 CHECK (floor_value >= 0),
    notes           TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX depreciation_models_organization_id_idx ON depreciation_models (organization_id);

CREATE TABLE depreciation_model_translations (
    id                     UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    depreciation_model_id  UUID NOT NULL REFERENCES depreciation_models(id) ON DELETE CASCADE,
    locale                 TEXT NOT NULL CHECK (locale IN ('en', 'fr', 'ar')),
    name                   TEXT NOT NULL CHECK (btrim(name) <> ''),
    CONSTRAINT depreciation_model_translations_unique UNIQUE (depreciation_model_id, locale)
);

-- --------------------------------------------------------------- products ----
CREATE TABLE products (
    id                     UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id        UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    code                   TEXT NOT NULL CHECK (code ~ '^[A-Z0-9]{1,8}$'),
    category_id            UUID NOT NULL REFERENCES categories(id) ON DELETE RESTRICT,
    manufacturer_id        UUID REFERENCES manufacturers(id) ON DELETE SET NULL,
    depreciation_model_id  UUID REFERENCES depreciation_models(id) ON DELETE SET NULL,
    model_code             TEXT,
    eol_type               TEXT NOT NULL DEFAULT 'individual'
                           CHECK (eol_type IN ('fixed_date', 'months_from_purchase',
                                               'months_from_warranty', 'individual')),
    eol_date               DATE,
    default_purchase_cost  NUMERIC(14, 2) CHECK (default_purchase_cost >= 0),
    default_currency       TEXT NOT NULL DEFAULT 'USD' CHECK (default_currency ~ '^[A-Z]{3}$'),
    default_supplier_id    UUID REFERENCES suppliers(id) ON DELETE SET NULL,
    min_quantity           INTEGER NOT NULL DEFAULT 0 CHECK (min_quantity >= 0),
    chip                   TEXT,
    operating_system       TEXT,
    notes                  TEXT,
    custom_fields_json     JSONB NOT NULL DEFAULT '{}'::jsonb,
    specs_json             JSONB NOT NULL DEFAULT '{}'::jsonb,
    is_archived            BOOLEAN NOT NULL DEFAULT FALSE,
    image_url              TEXT,
    created_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT products_org_code_key UNIQUE (organization_id, code),
    -- A fixed EOL date is meaningless without the date itself.
    CONSTRAINT products_eol_fixed_date_required
        CHECK (eol_type <> 'fixed_date' OR eol_date IS NOT NULL)
);

CREATE INDEX products_organization_id_idx ON products (organization_id);
CREATE INDEX products_category_id_idx ON products (organization_id, category_id);
CREATE INDEX products_archived_idx ON products (organization_id, is_archived);

CREATE TABLE product_translations (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    product_id UUID NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    locale     TEXT NOT NULL CHECK (locale IN ('en', 'fr', 'ar')),
    name       TEXT NOT NULL CHECK (btrim(name) <> ''),
    CONSTRAINT product_translations_unique UNIQUE (product_id, locale)
);

-- Users get a home location so a user-assigned asset inherits it.
ALTER TABLE users ADD COLUMN location_id UUID REFERENCES locations(id) ON DELETE SET NULL;
CREATE INDEX users_location_id_idx ON users (location_id);

-- ----------------------------------------------------------------- assets ----
CREATE TABLE assets (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    asset_tag       TEXT NOT NULL,
    product_id      UUID NOT NULL REFERENCES products(id) ON DELETE RESTRICT,
    status_id       UUID NOT NULL REFERENCES statuses(id) ON DELETE RESTRICT,
    supplier_id     UUID REFERENCES suppliers(id) ON DELETE SET NULL,
    location_id     UUID REFERENCES locations(id) ON DELETE SET NULL,
    department_id   UUID NOT NULL REFERENCES departments(id) ON DELETE RESTRICT,
    assigned_user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    name            TEXT NOT NULL CHECK (btrim(name) <> ''),
    serial_number   TEXT,
    warranty_date   DATE,
    order_number    TEXT,
    purchase_date   DATE,
    purchase_cost   NUMERIC(14, 2) CHECK (purchase_cost >= 0),
    currency        TEXT NOT NULL DEFAULT 'USD' CHECK (currency ~ '^[A-Z]{3}$'),
    schedule_audit  TIMESTAMPTZ,
    notes           TEXT,
    image_url       TEXT,
    -- Optional hardware subset (phones, tablets, cellular routers).
    imei_1          TEXT,
    imei_2          TEXT,
    mdm_enrolled    BOOLEAN NOT NULL DEFAULT FALSE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT assets_org_tag_key UNIQUE (organization_id, asset_tag)
);

CREATE INDEX assets_organization_id_idx ON assets (organization_id);
CREATE INDEX assets_status_id_idx ON assets (organization_id, status_id);
CREATE INDEX assets_location_id_idx ON assets (organization_id, location_id);
CREATE INDEX assets_department_id_idx ON assets (organization_id, department_id);
CREATE INDEX assets_product_id_idx ON assets (organization_id, product_id);
CREATE INDEX assets_assigned_user_id_idx ON assets (organization_id, assigned_user_id);

-- Serial numbers are unique per tenant when present (case-insensitive);
-- a blank string is treated as absent.
CREATE UNIQUE INDEX assets_org_serial_key
    ON assets (organization_id, lower(serial_number))
    WHERE serial_number IS NOT NULL AND btrim(serial_number) <> '';
CREATE UNIQUE INDEX assets_org_imei1_key
    ON assets (organization_id, imei_1)
    WHERE imei_1 IS NOT NULL AND btrim(imei_1) <> '';

-- Per (department, product) tag counter: COMM.AST.TLPH.0001, .0002, ...
CREATE TABLE asset_sequences (
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    department_id   UUID NOT NULL REFERENCES departments(id) ON DELETE CASCADE,
    product_id      UUID NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    last_value      INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (organization_id, department_id, product_id)
);

-- Append-only custody history. `assets` keeps the current holder; this table
-- explains how it got there.
CREATE TABLE asset_assignments (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id     UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    asset_id            UUID NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    action              TEXT NOT NULL CHECK (action IN ('assign', 'return', 'transfer')),
    assigned_to_user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    from_user_id        UUID REFERENCES users(id) ON DELETE SET NULL,
    from_location_id    UUID REFERENCES locations(id) ON DELETE SET NULL,
    to_location_id      UUID REFERENCES locations(id) ON DELETE SET NULL,
    from_department_id  UUID REFERENCES departments(id) ON DELETE SET NULL,
    to_department_id    UUID REFERENCES departments(id) ON DELETE SET NULL,
    from_status_id      UUID REFERENCES statuses(id) ON DELETE SET NULL,
    to_status_id        UUID REFERENCES statuses(id) ON DELETE SET NULL,
    notes               TEXT,
    performed_by_user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX asset_assignments_asset_idx
    ON asset_assignments (organization_id, asset_id, created_at DESC);

-- ----------------------------------------------------------- permissions ----
INSERT INTO permissions (code, description) VALUES
    ('lookups.read',   'View lookups (categories, manufacturers, statuses, ...)'),
    ('lookups.write',  'Create, update and delete lookups'),
    ('products.read',  'View the product catalog'),
    ('products.write', 'Create, update and archive products'),
    ('assets.read',    'View assets'),
    ('assets.write',   'Create, update and delete assets'),
    ('assets.assign',  'Assign, return and transfer assets')
ON CONFLICT (code) DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code IN (
    'lookups.read', 'lookups.write',
    'products.read', 'products.write',
    'assets.read', 'assets.write', 'assets.assign'
)
WHERE r.code = 'admin'
ON CONFLICT DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code IN (
    'lookups.read', 'lookups.write', 'products.read', 'products.write', 'assets.read', 'assets.assign'
)
WHERE r.code = 'manager'
ON CONFLICT DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code IN ('lookups.read', 'products.read', 'assets.read', 'assets.assign')
WHERE r.code = 'technician'
ON CONFLICT DO NOTHING;

INSERT INTO role_permissions (role_id, permission_id)
SELECT r.id, p.id
FROM roles r
JOIN permissions p ON p.code IN ('lookups.read', 'products.read', 'assets.read')
WHERE r.code = 'viewer'
ON CONFLICT DO NOTHING;
