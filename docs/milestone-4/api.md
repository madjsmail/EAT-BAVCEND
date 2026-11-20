# Milestone 4 — API Reference

Same envelope, auth, i18n and PATCH semantics as Milestone 3 (see
`docs/milestone-3/api.md`). All routes are mounted under `/v1/api`.
Permissions: `maintenances.read|write`, `attachments.read|write`,
`audit.read`, `dashboard.read`.

## Maintenance & RMA

`GET /maintenances?q=&asset_id=&status=&maintenance_type=&priority=&page=&per_page=`
(paginated, default 25, max 100).

```json
{
  "asset_id": "uuid",
  "title": "Screen flickering",
  "description": "Panel flickers below 40% brightness",
  "maintenance_type": "corrective",
  "priority": "high",
  "supplier_id": "uuid",
  "rma_reference": "RMA-2026-0042",
  "assigned_to_user_id": "uuid",
  "scheduled_at": "2026-02-04T09:00:00Z",
  "cost": 85.00,
  "currency": "USD"
}
```

`maintenance_type`: `preventive | corrective | inspection`.
`priority`: `low | medium | high | critical`. `status` is server-managed:
`open | in_progress | completed | cancelled`.

Response item:

```json
{
  "id": "uuid",
  "asset_id": "uuid",
  "asset_tag": "IT.AST.LAP.0004",
  "asset_name": "Dell Latitude 5440",
  "title": "Keyboard replacement",
  "description": null,
  "maintenance_type": "corrective",
  "status": "open",
  "priority": "high",
  "supplier_id": "uuid",
  "supplier_name": "Dell Support",
  "rma_reference": "RMA-2026-0042",
  "assigned_to_user_id": "uuid",
  "assigned_to_user_name": "Linus Torvalds",
  "previous_status_id": "uuid",
  "previous_status_name": "Deployed",
  "reported_at": "2026-02-01T10:15:00Z",
  "scheduled_at": null,
  "started_at": null,
  "completed_at": null,
  "completion_notes": null,
  "cost": 85.00,
  "currency": "USD",
  "created_by_user_id": "uuid",
  "created_by_user_name": "Ada Lovelace",
  "created_at": "2026-02-01T10:15:00Z",
  "updated_at": "2026-02-01T10:15:00Z"
}
```

- `POST /maintenances` — opening a ticket stores the asset's current status in
  `previous_status_id`, moves the asset to **Being Repaired**, and returns
  `409` when the asset already has an `open` / `in_progress` ticket.
- `GET /maintenances/{id}`.
- `PATCH /maintenances/{id}` — same body as create (all optional, `null`
  clears a field). `409` when the ticket is `completed` / `cancelled`.
- `POST /maintenances/{id}/start` — `open` → `in_progress`, sets `started_at`.
- `POST /maintenances/{id}/complete` — body
  `{ "completion_notes": "...", "cost": 85.00, "restore_status_id": null }`;
  restores `previous_status_id` (or `restore_status_id`), sets `completed_at`.
- `POST /maintenances/{id}/cancel` — body `{ "reason": "..." }`; restores the
  asset status.
- `DELETE /maintenances/{id}` — only `open` or `cancelled` (`409` otherwise);
  restores the asset status when the ticket was open.

Asset status transitions are audited as `maintenance_opened` /
`maintenance_closed`.

## Attachments

Metadata in PostgreSQL, bytes in Garage. Max **20 MB** per file. Allowed MIME:
`image/*`, `application/pdf`, `text/*`, `application/zip`, Word/Excel
(legacy and OOXML).

`POST /attachments` — `multipart/form-data`:

| Field | Type | Notes |
| --- | --- | --- |
| `attachable_type` | text | `asset`, `product`, `maintenance`, `license`, `accessory`, `consumable` |
| `attachable_id` | uuid | must exist in the caller's organization |
| `category` | text | `invoice`, `warranty`, `damage_photo`, `handover_receipt`, `other` (default) |
| `file` | file | the binary payload |

Response item:

```json
{
  "id": "uuid",
  "attachable_type": "asset",
  "attachable_id": "uuid",
  "category": "invoice",
  "file_name": "invoice-2026-014.pdf",
  "mime_type": "application/pdf",
  "size_bytes": 21451,
  "uploaded_by_user_id": "uuid",
  "uploaded_by_user_name": "Ada Lovelace",
  "created_at": "2026-02-01T10:15:00Z"
}
```

- `GET /attachments?attachable_type=&attachable_id=&category=&page=&per_page=`
  (paginated, default 25, max 100).
- `GET /attachments/{id}/url` → `{ "attachment_id": "uuid", "url": "https://...",
  "expires_in": 900 }` — presigned GET, valid 15 minutes. Do not cache it.
- `DELETE /attachments/{id}` — removes the row and the object.

Errors: `400` with `code: "unsupported_type"` for a disallowed MIME,
`code: "required"` for an empty/missing file, `code: "too_large"` above 20 MB,
`code: "not_found"` when the target row is missing.

## Audit Log Explorer

`GET /audit-logs` — read-only, requires `audit.read` (admin/manager).

| Query param | Notes |
| --- | --- |
| `actor_id` | uuid |
| `auditable_type` | singular slug: `asset`, `product`, `maintenance`, `license`, `consumable`, `accessory`, `user`, `department`, ... (alias `entity_type`) |
| `auditable_id` | uuid (alias `entity_id`) |
| `event` | repeatable (`event=a&event=b`, `event[]=a`, `events=a,b`) — all accepted |
| `date_from` / `date_to` | RFC 3339 or `YYYY-MM-DD` (end of day for `date_to`) |
| `search` | free text, matches `diff`, `old_values`, `new_values`, `metadata` |
| `page` / `per_page` | default 25, **max 100** |

`400` (`field: "date_from"`, `code: "invalid"`) when `date_from > date_to`.

Response item:

```json
{
  "id": "uuid",
  "event": "updated",
  "auditable_type": "asset",
  "auditable_id": "uuid",
  "actor_id": "uuid",
  "actor_name": "Grace Hopper",
  "actor_email": "manager@demo.local",
  "actor_ip": "198.51.100.7",
  "actor_user_agent": "Mozilla/5.0 ...",
  "old_values": { "status_id": "uuid", "priority": "high" },
  "new_values": { "status_id": "uuid", "priority": "critical" },
  "diff": { "priority": { "old": "high", "new": "critical" } },
  "changes": [{ "field": "priority", "old": "high", "new": "critical" }],
  "metadata": { "source": "trigger" },
  "created_at": "2026-02-01T10:15:00Z"
}
```

- `actor_name` is resolved server-side; it is `"System"` when there is no
  actor. **The timeline must render this field** (not the raw UUID).
- `changes` is the flattened `diff` (`[{ field, old, new }]`); use it for the
  old → new rows.
- Events: `created`, `updated`, `deleted`, `assigned`, `returned`,
  `transferred`, `issued`, `restocked`, `adjusted`, `revoked`, `login`,
  `login_failed`, `logout`, `invited`, `accepted`, `status_changed`,
  `maintenance_opened`, `maintenance_closed`, `seeded`, `bootstrapped`,
  `cross_access_denied`, `session_revoked`, `sessions_revoked`.
- Pagination metadata: `page`, `per_page`, `total`, `total_pages`.

## Executive Dashboard

All counts and sums are computed in SQL. Requires `dashboard.read` (all roles).

`GET /dashboard/asset-status`

```json
[{ "status_id": "uuid", "status": "Deployed", "code": "deployed",
   "color_code": "#10B981", "status_type": "deployed", "total": 4 }]
```

Unknown or unused statuses are returned with `total: 0`.

`GET /dashboard/expirations`

```json
{
  "warranties": { "expired": 1, "within_30_days": 2, "within_60_days": 2, "within_90_days": 1 },
  "licenses":   { "expired": 0, "within_30_days": 1, "within_60_days": 1, "within_90_days": 2 }
}
```

Buckets are cumulative (`within_60_days` includes the 30-day window).

`GET /dashboard/low-stock`

```json
[{ "kind": "consumable", "id": "uuid", "name": "AA Batteries (pack of 4)",
   "on_hand": 2, "threshold": 6, "shortfall": 4 }]
```

`kind` is `consumable` (quantity on hand ≤ alert) or `accessory` (available
≤ minimum). Sorted by `shortfall` descending.

`GET /dashboard/fleet-value`

```json
{
  "by_manufacturer": [{ "id": "uuid", "name": "Dell", "currency": "USD", "assets": 4, "total_value": 5800.00 }],
  "by_department":   [{ "id": "uuid", "name": "IT",   "currency": "USD", "assets": 4, "total_value": 5800.00 }],
  "totals":          [{ "currency": "USD", "assets": 8, "total_value": 9200.00 }]
}
```

Groups are split per currency — never sum across currencies in the UI.
