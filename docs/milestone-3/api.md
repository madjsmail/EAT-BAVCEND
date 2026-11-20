# Milestone 3 — API Reference

Same envelope, auth, i18n and PATCH semantics as Milestone 2 (see
`docs/milestone-2/api.md`). Every localized row returns `name` +
`translations: { en, fr, ar }`. Permissions:
`accessories.read|write|assign`, `consumables.read|write|issue`,
`licenses.read|write|assign`.

## Accessories

`GET /accessories?q=&category_id=&manufacturer_id=&is_archived=&page=&per_page=`
(paginated, default 25, max 100).

```json
{
  "translations": { "en": "USB-C Charger 65W", "fr": "Chargeur USB-C 65W", "ar": "شاحن USB-C 65 واط" },
  "category_id": "uuid",
  "manufacturer_id": "uuid",
  "supplier_id": "uuid",
  "location_id": "uuid",
  "model_number": "65W-GAN",
  "total_quantity": 40,
  "min_quantity": 10,
  "purchase_cost": 49.00,
  "currency": "USD",
  "purchase_date": "2026-01-15",
  "order_number": "PO-2026-014",
  "notes": null,
  "image_url": null,
  "is_archived": false
}
```

Response adds `available_quantity`, `assigned_quantity`
(`total - available`), resolved `category_name`, `manufacturer_name`,
`supplier_name`, `location_name`.

- `POST /accessories` — `available_quantity` starts at `total_quantity`.
- `GET|PATCH|DELETE /accessories/{id}`.
- `PATCH total_quantity` — increasing adds the delta to available; decreasing is
  rejected (400) below the assigned count.

### Assign / return

`POST /accessories/{id}/assign`

```json
{ "user_id": "uuid", "quantity": 5, "notes": "onboarding kit" }
{ "asset_id": "uuid", "quantity": 2, "notes": "charger paired with laptop" }
```

Exactly one of `user_id` / `asset_id`. Rejects with 409 when fewer units are
available, 400 for `quantity <= 0` or a cross-tenant target.

`GET /accessories/{id}/assignments` — newest first, includes
`quantity`, `returned_quantity`, `remaining_quantity`, `asset_tag`, user names.

`POST /accessories/{id}/assignments/{assignment_id}/return`

```json
{ "quantity": 2, "notes": "returned charger" }
```

`quantity` defaults to the outstanding amount; partial returns leave the row
open, full returns set `returned_at`. Over-return is a 400.

Both movements write `accessory.assign` / `accessory.return` audit rows.

## Consumables

`GET /consumables?q=&category_id=&low_stock=true&is_archived=&page=&per_page=`

Response includes `quantity_on_hand`, `min_stock_alert`, `is_low_stock`, plus
resolved lookup names.

- `GET /consumables/alerts` — non-archived rows where
  `quantity_on_hand <= min_stock_alert`, most short first.
- `POST /consumables` — `quantity_on_hand` is the opening stock and is recorded
  as a `restock` ledger row.
- `GET|PATCH|DELETE /consumables/{id}` — stock is **not** editable via PATCH.
- `GET /consumables/{id}/transactions` — newest first, includes
  `transaction_type`, `quantity`, `user_name`, `performed_by_user_name`.

`POST /consumables/{id}/transactions`

```json
{ "transaction_type": "issue", "quantity": 2, "user_id": "uuid", "notes": "helpdesk" }
{ "transaction_type": "restock", "quantity": 10 }
{ "transaction_type": "adjustment", "quantity": -1, "notes": "damaged unit" }
```

`restock`/`issue` require `quantity > 0`; `adjustment` is a signed delta
(non-zero). The balance is locked and updated atomically; an issue that would go
negative returns 409. Audited as `consumable.restock|issue|adjustment`.

## Licenses

`GET /licenses?q=&expiring_in_days=30&is_archived=&page=&per_page=`
(`expiring_in_days` = not-yet-expired licenses within the next N days).

```json
{
  "translations": { "en": "Adobe Acrobat Pro", "fr": "Adobe Acrobat Pro", "ar": "أدوبي أكروبات برو" },
  "manufacturer_id": "uuid",
  "supplier_id": "uuid",
  "license_type": "subscription",
  "total_seats": 10,
  "purchase_date": "2026-01-01",
  "expiration_date": "2026-12-15",
  "purchase_cost": 1800.00,
  "currency": "USD",
  "order_number": "PO-2026-020",
  "product_key": "XXXX-XXXX",
  "notes": null,
  "is_archived": false
}
```

Response adds `active_seats`, `available_seats`, `days_until_expiry`
(negative = expired), `expiry_bucket` (`expired`, `30`, `60`, `90`, or null),
and resolved names. `license_type` ∈
`named`, `concurrent`, `subscription`, `perpetual`.

`GET /licenses/alerts` — buckets:

```json
{
  "expired": [],
  "within_30_days": [],
  "within_60_days": [],
  "within_90_days": []
}
```

Buckets are non-overlapping: 0–30, 31–60, 61–90 days.

- `POST /licenses`, `GET|PATCH|DELETE /licenses/{id}`.
- `PATCH total_seats` below `active_seats` → 400.

### Seats

`GET /licenses/{id}/seats` — full history (active + revoked), resolved user and
asset names.

`POST /licenses/{id}/seats/allocate`

```json
{ "user_id": "uuid", "notes": "design team" }
{ "asset_id": "uuid", "notes": "shared workstation" }
```

Exactly one target. Locks the license, rejects with 409 when
`active_seats >= total_seats`, 409 on a duplicate live seat for the same target.

`POST /licenses/{id}/seats/{seat_id}/revoke`

```json
{ "notes": "offboarded" }
```

Body optional. Frees the seat and keeps the row with `revoked_at` +
`revoked_by_user_name`. Audited as `license.assign` / `license.revoke`.

## Unified 360° view

`GET /assets/{id}/360`

```json
{
  "asset": { "id": "uuid", "asset_tag": "COMM.AST.LAP.0001", "...": "same shape as GET /assets/{id}" },
  "accessories": [ { "id": "uuid", "asset_id": "uuid", "quantity": 2, "remaining_quantity": 2, "...": "..." } ],
  "consumables": [ { "id": "uuid", "transaction_type": "issue", "quantity": 1, "user_id": "uuid", "...": "..." } ],
  "licenses": [ { "id": "uuid", "license_id": "uuid", "asset_id": "uuid", "user_id": null, "...": "..." } ],
  "custody_history": [ { "action": "assign", "from_location_id": "uuid", "to_location_id": "uuid", "...": "..." } ],
  "counts": { "accessories": 1, "consumables": 1, "licenses": 2, "custody_events": 3 }
}
```

- `accessories` = live assignments paired with the **asset**.
- `consumables` = `issue` transactions for the asset's **assigned user**
  (consumables are people-scoped).
- `licenses` = active seats held by the **asset** plus active seats held by the
  asset's user.
- `custody_history` = `asset_assignments`, newest first.
- The handler issues the independent queries concurrently (`tokio::try_join!`);
  no per-row N+1.

## Errors (new cases)

| Status | Code | Case |
| --- | --- | --- |
| 400 | `invalid` | quantity ≤ 0, over-return, negative `total_seats` |
| 400 | `invalid` | `total_seats` below active seats, `total_quantity` below assigned |
| 409 | — | insufficient accessory stock / consumable stock / all seats allocated |
| 409 | — | duplicate live seat for the same user or asset |
| 404 | — | cross-tenant ids (assets, users, consumables, licenses) |
