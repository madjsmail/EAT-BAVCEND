# Milestone 3 — Accessories, Consumables & Software Licenses

Third backend slice: bulk accessories, consumable stock, software seats, and a
single 360° payload for the asset detail page.

## What shipped

| Area | Delivered |
| --- | --- |
| Accessories | CRUD, bulk quantities, assignments to a user **or** a parent asset, partial returns |
| Consumables | CRUD, append-only ledger (`restock`/`issue`/`adjustment`), low-stock flag + alerts feed |
| Licenses | CRUD, seat allocation to a user **or** asset, revoke history, hard seat cap |
| Expiry | `days_until_expiry`, `expiry_bucket` (`expired`/`30`/`60`/`90`), `/licenses/alerts` |
| 360° view | `GET /assets/{id}/360` — core asset + paired accessories + issued consumables + active seats + custody history |
| Seeding | 4 accessories, 4 consumables (2 low stock), 5 licenses (25/55/75/88/300 days) |

## Layout

```text
src/modules/
├── accessories/{dto,repo,handlers,mod}.rs
├── consumables/{dto,repo,handlers,mod}.rs
├── licenses/{dto,repo,handlers,mod}.rs
└── assets/            + asset_360 handler and Asset360Response
migrations/20260101000004_accessories_consumables_licenses.sql
tests/milestone3.rs    5 acceptance tests
```

## Concurrency rules

1. **Accessories** — assignment runs `SELECT ... FOR UPDATE` on the accessory,
   checks `available_quantity >= quantity`, then decrements. Returns add the
   units back; partial returns keep the assignment row open.
2. **Consumables** — every movement locks the consumable, computes
   `quantity_on_hand + delta`, rejects negatives with 409, then writes the
   ledger row and the new balance in one transaction.
3. **License seats** — allocation locks the license row before counting active
   seats. The count check plus the row lock (and the partial unique indexes)
   make `active_seats > total_seats` impossible.

## Run

```bash
cargo run -- seed --org-slug acme
cargo test
```

Seeder additions (per organization):

```text
accessories=4  consumables=4  licenses=5
```

Low stock on purpose: `HP 26A Toner Cartridge` (3/5) and
`AA Batteries (4-pack)` (2/6). License expiry windows: Adobe Acrobat Pro 25d,
Zoom Workplace Pro 55d, Slack Business+ 75d, JetBrains 88d, Microsoft 365 300d.
