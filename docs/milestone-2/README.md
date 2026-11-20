# Milestone 2 — Core Lookups, i18n & Universal Asset Registry

Backend slice that gives the frontend everything needed to render lookup
management, the product catalog, the asset registry, custody workflows and
printable labels.

## What shipped

| Area | Delivered |
| --- | --- |
| Localization | `*_translations` tables (EN/FR/AR), `Accept-Language` resolver, `name` + `translations` in every response |
| Lookups | Categories, Manufacturers, Statuses, Departments, hierarchical Locations, Suppliers, Depreciation models — full CRUD |
| Catalog | Products with codes, EOL strategies, specs/custom JSON, archive toggle |
| Assets | Universal registry (`asset_tag`, `serial_number`, optional IMEI/MDM), filters, quick-scan |
| Labels | QR / Code128 rendered to JSON, SVG or PNG |
| Custody | Assign / return / transfer with `asset_assignments` + `audit_logs` in one transaction |
| Seeding | `cargo run -- seed --org-slug <slug>` — idempotent EN/FR/AR defaults |

## Layout

```text
src/modules/
├── i18n/mod.rs        Locale extractor + pick/validate helpers
├── common.rs          tenant-FK guard, code/enum helpers, derive_code
├── lookups/           7 lookup families (dto, repo, handlers, router)
├── products/          catalog
└── assets/            registry, custody, labels, quick-scan
src/seed/mod.rs        default data seeder
migrations/20260101000003_core_lookups_assets.sql
tests/milestone2.rs    4 acceptance tests
```

## Running

```bash
cargo run -- seed --org-slug acme     # migrations run first, then seed
cargo test                            # unit + milestone 1 + milestone 2
```

Seeder output (per organization):

```text
departments=4  categories=4  manufacturers=4  statuses=7
locations=4    suppliers=2   depreciation_models=2  products=3
```

Seeded statuses: Archived, Being Repaired, Broken, Deployed, Lost or Stolen,
Pending, Ready to Deploy. Categories: Accessory, Asset, Consumable, Component.
Departments: COMM, IT, FIN, HR. Locations: HQ → HQ-WH, SITE-SFAX,
SITE-SOUSSE. Products: Dell Latitude 5440 (`LAP`), Cisco IP Phone 8841
(`TLPH`), Jabra Evolve2 65 (`HDST`).

## Decisions

- Per-organization lookups; `organization_id` is never accepted from the client.
- No JSON columns for names — translations are relational so admins edit one
  locale without rewriting a blob.
- Facility names in the requested locale fall back to English, then to any
  stored locale.
- Asset tags are generated per `(department, product)` with an atomic
  `asset_sequences` upsert; explicit tags are allowed and must be unique.
- Assigning to a user inherits `users.location_id`; a user without a location
  is rejected (400) rather than silently parking the asset.
- Custody history is append-only and always paired with an audit row.

See `api.md` for the exact contracts and `docs/database_schema_description.md`
for column-level rules.
