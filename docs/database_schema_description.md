# Database Schema — Description

Companion to `database_schema_mermaid_erd.md`. Migrations:
`0001_init.sql` (identity/RBAC/audit), `0002_seed_rbac.sql` (permission + role
seed), `0003_core_lookups_assets.sql` (lookups, catalog, assets, custody).

## Conventions

- All primary keys are `UUID DEFAULT gen_random_uuid()`.
- Every tenant table has `organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE`; **queries must always filter on it**.
- `created_at` / `updated_at` are `TIMESTAMPTZ NOT NULL DEFAULT now()`; updates set `updated_at = now()` explicitly.
- Localized names live in `<entity>_translations` tables — never in JSON.
  - Columns: `id`, `<entity>_id FK ... ON DELETE CASCADE`, `locale TEXT CHECK (locale IN ('en','fr','ar'))`, `name TEXT CHECK (btrim(name) <> '')`.
  - `UNIQUE (<entity>_id, locale)` and an index on the parent FK.
- Deletes of referenced lookups are blocked (`ON DELETE RESTRICT`); deleting a
  translation row or the parent lookup cascades its translations.

## Identity & RBAC (Milestone 1)

| Table | Constraints / rules |
| --- | --- |
| `organizations` | `slug` unique; soft-free (hard delete cascades all tenants data). |
| `permissions` | Global, `code` unique (`assets.read`, `lookups.write`, ...). |
| `roles` | `code` unique; `organization_id` NULL = system role shared by tenants; `is_system` marks seeded roles. |
| `role_permissions` | PK `(role_id, permission_id)`. |
| `users` | `email = lower(email)`; `UNIQUE (organization_id, email)`; `status IN ('invited','active','suspended','offboarded')`; `preferred_locale IN ('en','fr','ar')` default `'en'`; `location_id` FK → `locations` (home location, `ON DELETE SET NULL`); `department_id` FK → `departments` (`SET NULL`); `manager_id` self-FK (`SET NULL`, `CHECK (manager_id <> id)`); `employee_id` unique per org case-insensitively when non-blank; `deleted_at` soft delete; `email_verified_at` set on activation. |
| `user_roles` | `UNIQUE (organization_id, user_id, role_id)`. |
| `invitations` | `token_hash` unique (SHA-256 only); `status IN ('pending','accepted','expired','revoked')`; at most one `pending` row per `(organization_id, email)` via partial unique index. |
| `sessions` | `token_hash` unique; `revoked_at` NULL = live; expiry checked in the auth extractor. |
| `audit_logs` | Append-only. `organization_id`/`actor_user_id` nullable (pre-auth/system events); `metadata JSONB`; indexed by `(organization_id, created_at DESC)`. |

## Lookups

### `departments` / `department_translations`
`code` `^[A-Z0-9]{1,8}$`, `UNIQUE (organization_id, code)`. Optional `email`,
`phone_number`. Code feeds asset tags.

### `categories` / `category_translations`
`code` `^[A-Z0-9]{1,8}$`, `UNIQUE (organization_id, code)`.
`category_type IN ('accessory','asset','consumable','component')`.
`skip_checkout_confirmation_emails` BOOLEAN default FALSE. `icon_url`.

### `manufacturers` / `manufacturer_translations`
No natural code — matched by English name at seed time. Optional `url`,
`support_url`, `support_phone`, `support_email`, `notes`, `logo_url`.

### `statuses` / `status_translations`
- `code` `^[a-z0-9_]{1,32}$`, `UNIQUE (organization_id, code)` — stable machine key used by custody transitions (`deployed`, `ready_to_deploy`).
- `status_type IN ('deployable','deployed','undeployable','pending','archived')` — fallback when a code is renamed.
- `color_code ~ '^#[0-9A-Fa-f]{6}$'`, default `#6B7280`.
- `show_in_nav` drives the navigation badge; list responses include `assets_count`.

### `locations` / `location_translations`
- `code` `^[A-Z0-9-]{1,16}$`, `UNIQUE (organization_id, code)`.
- `parent_id` self-FK `ON DELETE RESTRICT`, `CHECK (parent_id <> id)`. Deeper
  cycles are rejected in `repo::check_location_parent` with a recursive CTE.
- `location_type IN ('site','building','floor','room','shelf')`, default `site`.
- `country_code` ISO 3166-1 alpha-2 (`^[A-Z]{2}$`), `latitude ∈ [-90,90]`,
  `longitude ∈ [-180,180]`, `is_active` default TRUE.
- `manager_user_id` must belong to the same organization (checked in repo).

### `suppliers` / `supplier_translations`
No code. `contact_name`, `email`, `phone_number`, `address`, `url`, `notes`.

### `depreciation_models` / `depreciation_model_translations`
`months INTEGER > 0`, `floor_value NUMERIC(14,2) >= 0`.

## Catalog

### `products` / `product_translations`
- `code` `^[A-Z0-9]{1,8}$`, `UNIQUE (organization_id, code)`.
- `category_id NOT NULL` (`RESTRICT`); `manufacturer_id`,
  `depreciation_model_id`, `default_supplier_id` optional (`SET NULL`).
- `eol_type IN ('fixed_date','months_from_purchase','months_from_warranty','individual')`,
  default `individual`; `CHECK (eol_type <> 'fixed_date' OR eol_date IS NOT NULL)`.
- `default_purchase_cost NUMERIC(14,2) >= 0`, `default_currency` `^[A-Z]{3}$`
  default `USD`.
- `min_quantity >= 0`; `chip`, `operating_system`, `model_code`, `notes`,
  `image_url`, `is_archived`.
- `custom_fields_json JSONB DEFAULT '{}'` (per-tenant extensions) and
  `specs_json JSONB DEFAULT '{}'` (typed hardware specs: CPU/RAM/IMEI-capable
  devices, furniture dimensions, vehicle plate...).

## Assets

### `assets`
| Column | Rule |
| --- | --- |
| `asset_tag` | `UNIQUE (organization_id, asset_tag)`; generated as `DEPT.CAT.PROD.NNNN` when omitted. |
| `product_id` | NOT NULL, `RESTRICT`. |
| `status_id` | NOT NULL, `RESTRICT`; defaults to the org's `deployable` status. |
| `department_id` | NOT NULL, `RESTRICT`. |
| `supplier_id`, `location_id`, `assigned_user_id` | Optional, `SET NULL`. |
| `name` | NOT NULL, `btrim(name) <> ''`. |
| `serial_number` | Unique per org case-insensitively, ignoring blanks (partial index). |
| `imei_1` | Unique per org when non-blank; `imei_2` free. |
| `warranty_date`, `purchase_date` | `DATE` nullable. |
| `purchase_cost` | `NUMERIC(14,2) >= 0`; `currency` `^[A-Z]{3}$` default `USD`. |
| `schedule_audit` | `TIMESTAMPTZ` nullable = next scheduled audit. |
| `mdm_enrolled` | BOOLEAN default FALSE. |
| `order_number`, `notes`, `image_url` | Optional text. |

### `asset_sequences`
PK `(organization_id, department_id, product_id)`, `last_value INTEGER`.
Incremented atomically with
`INSERT ... ON CONFLICT DO UPDATE SET last_value = asset_sequences.last_value + 1 RETURNING last_value`,
so concurrent creates never reuse a tag.

### `asset_assignments`
Append-only custody log:
`action IN ('assign','return','transfer')`, `assigned_to_user_id`,
`from_user_id`, `from_location_id`/`to_location_id`,
`from_department_id`/`to_department_id`, `from_status_id`/`to_status_id`,
`notes`, `performed_by_user_id`, `created_at`.
Indexed by `(organization_id, asset_id, created_at DESC)`.

## Business rules enforced in code

1. **Assign**: exactly one of `user_id` / `location_id`. Assigning to a user
   requires `users.location_id`; the asset inherits it, otherwise 400
   `location_required`. Status defaults to `deployed`.
2. **Return**: clears `assigned_user_id`; status defaults to `ready_to_deploy`.
3. **Transfer**: moves location, optionally department/status.
4. Every transition locks the asset `FOR UPDATE`, writes `asset_assignments`
   **and** `audit_logs` in one transaction, then commits.
5. Seeded permissions: `lookups.read/write`, `products.read/write`,
   `assets.read/write/assign`. Admin gets all; manager all except
   `assets.write`; technician read + assign; viewer read.

## Accessories, consumables & licenses (Milestone 3)

Migration: `20260101000004_accessories_consumables_licenses.sql`.

### `accessories` / `accessory_translations`
- Optional links: `category_id`, `manufacturer_id`, `supplier_id`, `location_id` (`ON DELETE SET NULL`).
- `total_quantity >= 0`; `CHECK (available_quantity BETWEEN 0 AND total_quantity)`.
- `min_quantity >= 0`, `purchase_cost >= 0`, `currency` ISO-4217, `is_archived`.
- Creating a row sets `available_quantity = total_quantity`; increasing `total_quantity`
  adds the delta to `available_quantity`; decreasing is rejected below the assigned
  count (`total_quantity - available_quantity`).

### `accessory_assignments`
- `quantity > 0`; `returned_quantity BETWEEN 0 AND quantity`.
- `CHECK (num_nonnulls(assigned_to_user_id, asset_id) = 1)` — exactly one target
  (employee **or** parent asset).
- `assigned_by_user_id`, `returned_by_user_id`, `returned_at`.
- Assignment: lock accessory row, require `available_quantity >= quantity`, decrement,
  insert row. Return (partial allowed): lock assignment row, add units back, set
  `returned_at` when fully returned. Both write `audit_logs`.

### `consumables` / `consumable_translations`
- Same optional lookups as accessories; `quantity_on_hand >= 0`, `min_stock_alert >= 0`.
- Low stock when `quantity_on_hand <= min_stock_alert` (computed in SQL, exposed as
  `is_low_stock`, filterable with `?low_stock=true`, dedicated `/consumables/alerts`).

### `consumable_transactions`
- Append-only ledger. `transaction_type IN ('restock','issue','adjustment')`.
- `CHECK (transaction_type = 'adjustment' OR quantity > 0)`; adjustments carry a
  signed delta and may be negative.
- Optional `user_id` (issue target; assets are not a consumable target),
  `performed_by_user_id`.
- Application rule: `SELECT quantity_on_hand ... FOR UPDATE`, reject when the resulting
  balance would be negative, write the transaction and the new balance in one transaction.
- Creating a consumable with opening stock inserts a `restock` row ("Opening stock").

### `licenses` / `license_translations`
- `license_type IN ('named','concurrent','subscription','perpetual')` (default `named`).
- `total_seats >= 0`, optional `expiration_date`, `purchase_date`, `purchase_cost`,
  `product_key`, `order_number`, `is_archived`.
- `active_seats` = count of seats with `revoked_at IS NULL`; `available_seats` derived.
- Expiry buckets computed from `expiration_date - CURRENT_DATE`: `expired` (<0), `30`,
  `60`, `90` (inclusive windows), otherwise null.

### `license_seats`
- `CHECK (num_nonnulls(user_id, asset_id) = 1)` — employee **or** asset.
- `status` is derived from `revoked_at`; revocation keeps the row for history.
- Partial unique indexes guarantee one live seat per `(license, user)` and
  `(license, asset)`.
- Allocation locks the license row, counts active seats, and rejects with 409 when
  `active_seats >= total_seats`. Lowering `total_seats` below `active_seats` is a 400.

### Permissions
`accessories.read/write/assign`, `consumables.read/write/issue`,
`licenses.read/write/assign`.
Admin: all. Manager: all. Technician: read + assign/issue. Viewer: read.

## Maintenance, attachments & audit explorer (Milestone 4)

Migrations: `0004_accessories_consumables_licenses.sql`,
`0005_audit_logs_explorer.sql`, `0006_maintenances_attachments.sql`.

### `asset_maintenances`

| Column | Rules |
| --- | --- |
| `asset_id` | FK → `assets` `ON DELETE CASCADE`; `(organization_id, asset_id)` filtered by status. |
| `title` | `CHECK (btrim(title) <> '')`. |
| `maintenance_type` | `preventive`, `corrective` (default), `inspection`. |
| `status` | `open` (default), `in_progress`, `completed`, `cancelled`. |
| `priority` | `low`, `medium` (default), `high`, `critical`. |
| `supplier_id` / `assigned_to_user_id` / `created_by_user_id` | nullable FKs, `ON DELETE SET NULL`. |
| `rma_reference` | free text RMA / case number. |
| `previous_status_id` | FK → `statuses`; status of the asset before the ticket opened. |
| `reported_at` / `scheduled_at` / `started_at` / `completed_at` | `TIMESTAMPTZ`. |
| `completion_notes` | free text, required on completion. |
| `cost` / `currency` | `NUMERIC(14,2) >= 0`, ISO-4217 currency (default `USD`). |

Business rules:

- Opening a ticket runs in a transaction, locks the asset
  (`SELECT ... FOR UPDATE`), rejects a second open ticket with `409`, stores
  `previous_status_id`, and moves the asset to `being_repaired` (fallback:
  the first `status_type = 'pending'` status).
- Completing or cancelling restores `previous_status_id` (or an explicitly
  supplied `restore_status_id`; fallback `ready_to_deploy`, else the first
  `deployable` status).
- `completed` / `cancelled` tickets are immutable (`PATCH` returns `409`);
  only `open` / `cancelled` tickets may be deleted.
- Indexes: `(organization_id, created_at DESC)`, `(organization_id, asset_id,
  created_at DESC)`, `(organization_id, status, created_at DESC)`, partial
  `assigned_to_user_id`; partial unique
  `asset_maintenances_one_open_per_asset (organization_id, asset_id) WHERE
  status IN ('open','in_progress')`.

### `attachments`

| Column | Rules |
| --- | --- |
| `attachable_type` | `asset`, `product`, `maintenance`, `license`, `accessory`, `consumable`. |
| `attachable_id` | `UUID NOT NULL`; target must exist in the same organization. |
| `category` | `invoice`, `warranty`, `damage_photo`, `handover_receipt`, `other` (default). |
| `file_name` | `CHECK (btrim(file_name) <> '')`. |
| `mime_type` | Sniffed from the multipart part; API allowlists image/pdf/text/zip/office types. |
| `size_bytes` | `BIGINT >= 0`; API cap 20 MB per file. |
| `storage_key` | `UNIQUE`; Garage object key `{organization_id}/{entity_type}/{entity_id}/{uuid}-{sanitized_filename}`. |
| `uploaded_by_user_id` | FK → `users`, `ON DELETE SET NULL`. |

Indexes: `(organization_id, attachable_type, attachable_id, created_at DESC)`
and a partial index on `uploaded_by_user_id`.

### `audit_logs` (extended)

New columns: `actor_user_agent TEXT`, `old_values JSONB`, `new_values JSONB`,
`diff JSONB`. Existing names are unchanged (`action`, `entity_type`,
`entity_id`, `metadata`, `ip_address`).

- **Capture:** the `audit_row_change()` trigger fires `AFTER INSERT OR UPDATE
  OR DELETE` on `assets`, `products`, `departments`, `categories`,
  `manufacturers`, `statuses`, `locations`, `suppliers`,
  `depreciation_models`, `accessories`, `consumables`, `licenses`, `users`,
  `asset_maintenances`, `attachments`.
  - `INSERT` → `action = 'created'`, `new_values` = full row.
  - `UPDATE` → `action = 'updated'`, `diff = {"<column>": {"old": …, "new": …}}`
    only for columns that actually changed (ignores `created_at`,
    `updated_at`, `last_login_at`; no row when nothing changed).
  - `DELETE` → `action = 'deleted'`, `old_values` = full row.
  - `entity_type` is the singular slug (`asset`, `product`, `maintenance`, …).
  - Actor attribution comes from transaction-local settings written by
    `repository::audit::bind_actor()`: `app.actor_user_id`, `app.request_ip`,
    `app.user_agent`.
- **Custom events** (`login`, `assigned`, `issued`, `maintenance_opened`,
  `maintenance_closed`, …) are inserted explicitly by handlers/repos and are
  not duplicated by the trigger.
- **Indexes:** `(organization_id, created_at DESC)`,
  `(organization_id, actor_user_id, created_at DESC)`,
  `(organization_id, entity_type, entity_id, created_at DESC)`,
  `(organization_id, action, created_at DESC)`, plus trigram GIN indexes on
  `diff::text`, `old_values::text`, `new_values::text` for free-text search
  (`pg_trgm` extension).

### Permissions added

`maintenances.read`, `maintenances.write`, `attachments.read`,
`attachments.write`, `audit.read`, `dashboard.read`.

## User directory & email-first auth (Milestone 5)

Migration: `20260101000007_users_directory_auth.sql`.

### `users` (extended)

- `employee_id TEXT` — internal HR id; unique per organization case-insensitively
  when non-blank (partial index `users_employee_id_key`).
- `job_title TEXT`, `department_id UUID` (FK → `departments`, `SET NULL`),
  `manager_id UUID` (self-FK, `SET NULL`, `CHECK (manager_id <> id)`).
- `email_verified_at TIMESTAMPTZ` — set by `user::activate` / `setup-account`.
- `deleted_at TIMESTAMPTZ` — soft delete; every user read filters it out.
- `status` now includes `offboarded`; `preferred_locale` is constrained to
  `('en','fr','ar')`.

### `auth_tokens`

Single-use email tokens for both invitations and magic logins.

| Column | Rule |
| --- | --- |
| `user_id` | FK → `users` `ON DELETE CASCADE`. |
| `token_hash` | `UNIQUE`; only SHA-256 hashes are stored. |
| `purpose` | `invite` (72 h) or `login` (1 h). |
| `expires_at`, `consumed_at` | Expiry checked on use; consumed tokens are kept for audit. |

Partial unique index `auth_tokens_one_live_idx (organization_id, user_id, purpose)`
`WHERE consumed_at IS NULL`; issuing a new token replaces the previous live row
(`repository::auth_token::replace_live`). Flow: `POST /auth/login-request`
always returns the same generic message, creates a token for known users and logs
the link; `POST /auth/setup-account` locks the token, validates expiry, activates
the user, consumes the token and issues a session cookie.

Password login (`POST /auth/login`) resolves the tenant from the user record, so
`organization_slug` is optional. When the same email exists in more than one
organization the request fails with `409 ambiguous_organization` (field
`organization_slug`) instead of guessing; emails are otherwise expected to be
unique across the install even though the schema only enforces
`UNIQUE (organization_id, email)`.

### `return_requests`

Offboarding worklist for custody held by the departing user.

| Column | Rule |
| --- | --- |
| `entity_type` | `asset`, `accessory`, `license`. |
| `entity_id` | Target row (polymorphic; no FK). |
| `quantity` | Accessories only, `> 0`; NULL for assets and licenses. |
| `status` | `pending` (default), `returned`, `cancelled`. |
| `requested_by` / `resolved_by` | FK → `users`, `SET NULL`. |
| `resolved_at`, `notes` | Resolution metadata. |

Partial unique index `return_requests_one_pending_idx
(organization_id, entity_type, entity_id) WHERE status = 'pending'`.
`POST /users/{id}/offboard` creates one pending row per asset in custody, open
accessory assignment (remaining quantity) and live license seat, reassigns direct
reports (a `new_manager_id` is required when reports exist), revokes sessions and
writes an audit row — all in one transaction.

### Import

`POST /users/import` accepts CSV or XLSX (`multipart/form-data`, field `file`;
first worksheet only for workbooks):
required `first_name,last_name,email`; optional `employee_id,job_title,
department,location,manager_email,preferred_locale,phone_number`. Unknown
departments are created (code derived from the name, capped at 8 chars). Each row
is its own transaction; imported users are `invited` with no password and get the
system `employee` role. Legacy `.xls` files are rejected with a 400.

### Permissions added

`users.create`, `users.delete`, `users.import`, `users.offboard`. Admin: all;
manager: create/import/offboard. System role `employee`: `organizations.read`,
`users.read`.
