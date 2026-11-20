# Milestone 2 — API Reference

Base path: `/v1/api`. Auth: session cookie `eat_session`
(`POST /v1/api/auth/login`). Every response uses the Milestone 1 envelope:

```json
{ "error": false, "status": 200, "message": "success", "data": {}, "pagination": null }
```

Lists that paginate add
`"pagination": { "page": 1, "per_page": 25, "total": 42, "total_pages": 2 }`.

Permissions: `lookups.read|write`, `products.read|write`,
`assets.read|write|assign` (granted per role in migration 3).

## i18n contract

All localized lookups/products return both shapes:

```json
{
  "id": "…",
  "code": "deployed",
  "name": "Déployé",
  "translations": { "en": "Deployed", "fr": "Déployé", "ar": "موزع" }
}
```

`name` is resolved from `Accept-Language` (`en`, `fr`, `ar`; regional values
collapse, e.g. `fr-CA` → `fr`, q-weights honoured). Fallbacks:
requested locale → user `preferred_locale` → `en` → any available locale.
`translations` always returns every stored locale.

Write bodies always send `translations`, an object with at least one non-empty
key from `en`/`fr`/`ar`:

```json
{ "translations": { "en": "Deployed", "fr": "Déployé", "ar": "موزع" } }
```

`PATCH` semantics: absent field = unchanged, explicit `null` = clear,
`translations` object = replace the whole map.

## Lookups

Unpaginated lists with `?q=` (case-insensitive search over all translations).
Each family has `GET|POST /lookups/<name>` and
`GET|PATCH|DELETE /lookups/<name>/{id}`.

| Resource | Path | Extra fields (request → response) |
| --- | --- | --- |
| Categories | `/lookups/categories` | `code?` (1-8 `A-Z0-9`, derived from EN name), `category_type` (`accessory`,`asset`,`consumable`,`component`), `skip_checkout_confirmation_emails`, `icon_url` |
| Manufacturers | `/lookups/manufacturers` | `url`, `support_url`, `support_phone`, `support_email`, `notes`, `logo_url` |
| Statuses | `/lookups/statuses` | `code?` (lowercase key), `status_type` (`deployable`,`deployed`,`undeployable`,`pending`,`archived`), `color_code` `#RRGGBB`, `show_in_nav`, `notes`; response adds `assets_count` |
| Departments | `/lookups/departments` | `code?` (1-8), `email`, `phone_number` |
| Locations | `/lookups/locations` | `code?` (1-16 incl. `-`), `parent_id`, `location_type` (`site`,`building`,`floor`,`room`,`shelf`), `address_line1/2`, `city`, `state_region`, `postal_code`, `country_code`, `latitude`, `longitude`, `phone_number`, `email`, `manager_user_id`, `notes`, `is_active` |
| Suppliers | `/lookups/suppliers` | `contact_name`, `email`, `phone_number`, `address`, `url`, `notes` |
| Depreciation models | `/lookups/depreciation-models` | `months` (>0), `floor_value` (≥0), `notes` |

Example:

```bash
curl -X POST localhost:8080/v1/api/lookups/statuses \
  -H 'content-type: application/json' -b 'eat_session=…' \
  -d '{"translations":{"en":"Deployed","fr":"Déployé","ar":"موزع"},
       "status_type":"deployed","color_code":"#10B981","show_in_nav":true}'
```

Location cycles and cross-tenant parents/managers are rejected with 400;
deletes of referenced rows return 409.

## Products

`GET /products` (paginated) supports `q`, `category_id`, `manufacturer_id`,
`is_archived`, `page`, `per_page` (≤100, default 25).
`POST /products`, `GET|PATCH|DELETE /products/{id}`.

Create body:

```json
{
  "translations": { "en": "Dell Latitude 5440", "fr": "Dell Latitude 5440", "ar": "ديل لاتيتود 5440" },
  "code": "LAP",
  "category_id": "uuid",
  "manufacturer_id": "uuid",
  "depreciation_model_id": "uuid",
  "model_code": "Latitude 5440",
  "eol_type": "months_from_purchase",
  "eol_date": null,
  "default_purchase_cost": 1450.00,
  "default_currency": "USD",
  "default_supplier_id": "uuid",
  "min_quantity": 0,
  "chip": "Intel Core i5-1335U",
  "operating_system": "Windows 11 Pro",
  "notes": null,
  "custom_fields_json": {},
  "specs_json": { "ram": "16GB", "storage": "512GB SSD" },
  "is_archived": false,
  "image_url": null
}
```

`eol_type=fixed_date` requires `eol_date`. Responses include resolved
`category_name`, `manufacturer_name`, `depreciation_model_name`,
`default_supplier_name`, plus `translations`/`name`.

## Assets

### Create — `POST /assets`

```json
{
  "product_id": "uuid",
  "department_id": "uuid",
  "name": "Finance laptop",
  "asset_tag": null,
  "status_id": null,
  "supplier_id": null,
  "location_id": null,
  "serial_number": "SN-001",
  "warranty_date": "2028-01-31",
  "order_number": "PO-2026-014",
  "purchase_date": "2026-01-31",
  "purchase_cost": 1450.00,
  "currency": "USD",
  "schedule_audit": "2026-12-01T09:00:00Z",
  "notes": null,
  "image_url": null,
  "imei_1": null,
  "imei_2": null,
  "mdm_enrolled": false
}
```

Omitting `asset_tag` generates `DEPT.CAT.PROD.NNNN` (per department+product
sequence, zero-padded to 4): `COMM.AST.LAP.0001`, `COMM.AST.LAP.0002`, ...
Omitting `status_id` uses the organization's `deployable` status.

### Read / update / delete

- `GET /assets` — paginated; filters `q` (tag/name/serial ILIKE),
  `status_id`, `location_id`, `department_id`, `product_id`,
  `assigned_user_id`, `page`, `per_page`.
- `GET /assets/{id}` — single asset with resolved names
  (`product_name`, `category_name`, `manufacturer_name`, `status_name`,
  `status_type`, `status_color_code`, `supplier_name`, `location_name`,
  `department_name`, `assigned_user_name`).
- `PATCH /assets/{id}`, `DELETE /assets/{id}` (audited).

### Quick scan — `GET /assets/scan?code=<value>`

Exact match on `asset_tag`, `serial_number`, `imei_1`, then loose tag match.
Returns a single asset or 404. Scan target for keyboard-wedge scanners and the
mobile camera.

### Labels — `GET /assets/{id}/label`

| Query | Values | Default |
| --- | --- | --- |
| `format` | `json`, `svg`, `png` | `json` |
| `symbology` | `qr`, `code128` | `qr` |
| `data` | `asset_tag`, `serial_number` | `asset_tag` |

`json` returns:

```json
{ "asset_id": "uuid", "asset_tag": "COMM.AST.LAP.0001", "serial_number": "SN-001",
  "name": "Finance laptop", "status_name": "Deployed", "location_name": "Headquarters",
  "department_name": "Communication", "assigned_user_name": null,
  "data": "COMM.AST.LAP.0001", "data_source": "asset_tag", "symbology": "qr" }
```

`svg` → `image/svg+xml`, `png` → `image/png` (printable, quiet zone included).

### Custody

`POST /assets/{id}/assign` — exactly one of:

```json
{ "user_id": "uuid", "notes": "onboarding" }
{ "location_id": "uuid", "notes": "stock" }
```

User assignment requires the user to have a home location; the asset inherits
it (400 `location_required` otherwise). Status defaults to `deployed`.
Optional `status_id` overrides.

`POST /assets/{id}/return` — `{ "location_id": "uuid?", "status_id": "uuid?", "notes": "..." }`.
Clears the holder, defaults status `ready_to_deploy`.

`POST /assets/{id}/transfer` — `{ "location_id": "uuid", "department_id": "uuid?", "status_id": "uuid?", "notes": "..." }`.

`GET /assets/{id}/assignments` — custody history, newest first, with resolved
user/location/department/status names.

Every mutation above writes `asset_assignments` **and** `audit_logs`
(`asset.create`, `asset.update`, `asset.delete`, `asset.assign`,
`asset.return`, `asset.transfer`) inside one transaction. The audit metadata
carries `from`/`to` user, location, department and status ids.

## Errors

| Status | Body `error_stack[0]` |
| --- | --- |
| 400 | `{ "field": "…", "code": "invalid\|required\|unknown_enum\|not_found\|location_required\|constraint", "message": "…" }` |
| 401 | session missing/expired |
| 403 | missing permission |
| 404 | `Resource not found` (also used for cross-tenant ids) |
| 409 | unique/FK conflict |
