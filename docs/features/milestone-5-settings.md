# Milestone 5 — Settings & Organization

Backend surface for self-service account settings and tenant-level organization
settings. Base URL: **`/v1/api`**. All endpoints require an authenticated session
cookie (`eat_session`, HttpOnly) and are scoped to the caller's organization.

## Overview

**M5-BE-01 — User profile, password & sessions**

- Read/update the caller's own profile (`/me`).
- Change password with current-password verification; every other session is
  revoked, the session that performed the change stays valid.
- List and revoke the caller's active sessions, or sign out all other devices.

**M5-BE-02 — Organization settings & logo**

- Read tenant settings: name, default locale, logo (`logo_object_key` plus a
  presigned GET URL) and typed `settings` JSONB.
- Update settings with permission `organizations.update`; partial updates are
  merged into the stored JSONB, validation errors return `422`.
- Logo upload uses a presigned S3 PUT (Garage); the client uploads directly and
  then persists the returned object key through `PATCH /organization`.

## Endpoints

| # | Method | Path | Auth | Permission | Request body | Response `data` | Errors |
| - | ------ | ---- | ---- | ---------- | ------------ | --------------- | ------ |
| 1 | GET | `/me` | session | authenticated | — | `UserProfile` | 401 |
| 2 | PATCH | `/me` | session | authenticated | `ProfileUpdate` | `UserProfile` | 401, 422 |
| 3 | POST | `/me/password` | session | authenticated | `PasswordChange` | `{ revoked_sessions: number }` | 401, 409, 422 |
| 4 | GET | `/me/sessions` | session | authenticated | — | `Session[]` | 401 |
| 5 | DELETE | `/me/sessions/{id}` | session | authenticated (owner) | — | `null` | 401, 404 |
| 6 | DELETE | `/me/sessions` | session | authenticated | — | `{ revoked: number }` | 401 |
| 7 | GET | `/organization` | session | `organizations.read` | — | `Organization` | 401, 403, 404 |
| 8 | PATCH | `/organization` | session | `organizations.update` | `OrganizationUpdate` | `Organization` | 401, 403, 404, 422 |
| 9 | POST | `/organization/logo/upload-url` | session | `organizations.update` | `LogoUploadUrlRequest` | `LogoUploadUrl` | 401, 403, 422 |

Roles seeded by default: `admin` has every permission; `manager` has
`organizations.read` only; `technician` and `viewer` have `organizations.read`.

### Response envelope

Every endpoint returns the project envelope:

```jsonc
{
  "error": false,
  "status": 200,
  "data": { /* endpoint payload */ },
  "message": "success"
}
```

Errors set `error: true`, `status`, `message`, and `error_stack` (array of
`{ field?, code, message }`). `data` / `pagination` / `error_stack` are omitted
when empty.

## TypeScript types

```ts
// GET /me
export interface UserProfile {
  id: string; // uuid
  organization_id: string; // uuid
  email: string;
  first_name: string;
  last_name: string;
  name: string; // `${first_name} ${last_name}`.trim()
  phone_number: string | null;
  avatar_url: string | null;
  status: "invited" | "active" | "suspended" | "offboarded";
  preferred_locale: "en" | "fr" | "ar";
  auth_provider: string;
  employee_id: string | null;
  job_title: string | null;
  roles: string[]; // role codes, e.g. ["admin"]
  role: string | null; // first entry of roles
  email_verified_at: string | null; // ISO 8601
  last_login_at: string | null; // ISO 8601
  created_at: string; // ISO 8601
  updated_at: string; // ISO 8601
}

// PATCH /me — only these fields are self-editable; others are ignored.
export interface ProfileUpdate {
  first_name?: string; // trimmed, 1..=100 chars
  last_name?: string; // trimmed, 1..=100 chars
  phone_number?: string; // <= 40 chars
  avatar_url?: string; // http(s) URL, <= 1000 chars
  preferred_locale?: "en" | "fr" | "ar";
}

// POST /me/password
export interface PasswordChange {
  current_password: string;
  new_password: string;
}

// GET /me/sessions
export interface Session {
  id: string; // uuid
  ip_address: string | null;
  user_agent: string | null;
  created_at: string; // ISO 8601
  last_seen_at: string; // ISO 8601, refreshed at most once per minute
  expires_at: string; // ISO 8601
  current: boolean; // true for the session making the request
  is_current: boolean; // alias of `current` (same value)
}

// organizations.settings (JSONB), defaults shown
export interface OrgSettings {
  asset_tag_prefix: string; // default "EAT-"
  asset_tag_padding: number; // default 4
  warranty_alert_days: number; // default 30
  license_alert_days: number; // default 30
  currency: string; // ISO 4217, default "USD"
  require_handover_signature: boolean; // default false
}

// PATCH /organization settings — all optional, merged into stored values.
// Unknown keys are rejected.
export type OrgSettingsUpdate = Partial<OrgSettings>;

// GET/PATCH /organization
export interface Organization {
  id: string; // uuid
  name: string;
  slug: string;
  default_locale: "en" | "fr" | "ar";
  logo_object_key: string | null;
  logo_url: string | null; // presigned GET, ~15 min; null when unset or signing unavailable
  settings: OrgSettings;
  created_at: string; // ISO 8601
  updated_at: string; // ISO 8601
}

export interface OrganizationUpdate {
  name?: string; // trimmed, 1..=120 chars
  default_locale?: "en" | "fr" | "ar";
  logo_object_key?: string; // must start with `${org.id}/`, <= 500 chars
  settings?: OrgSettingsUpdate;
}

// POST /organization/logo/upload-url
export interface LogoUploadUrlRequest {
  file_name: string; // 1..=180 chars, sanitized server-side
  content_type: "image/png" | "image/jpeg" | "image/webp";
}

export interface LogoUploadUrl {
  object_key: string; // `${organization_id}/logo/${organization_id}/{uuid}-{file}`
  upload_url: string; // presigned S3 PUT, valid 900 s
  expires_in_seconds: number; // 900
  max_bytes: number; // 2097152 (2 MiB)
}
```

## Auth notes

- Authentication is a stateful session cookie; there is no JWT and no token in
  the body. `401` means missing/expired/revoked session.
- `403` is returned by the RBAC guard when the session lacks the permission.
  `GET /organization` needs `organizations.read`; all organization writes and
  the logo upload URL need `organizations.update`.
- Profile and session endpoints only touch the caller's own rows. Deleting a
  session that belongs to someone else (or does not exist) returns `404`.
- Password change revokes every other session (`revoked_at = now()`), keeping the
  current one. The response reports how many were revoked.
- `DELETE /me/sessions` does the same across all devices but keeps the current
  session, so the UI stays logged in.
- `last_seen_at` is refreshed lazily by the auth extractor at most once per
  minute per session; it is not updated on every request in the database.

## Logo upload flow

1. `POST /organization/logo/upload-url` with `{ file_name, content_type }`.
2. Server validates MIME/name, builds the key under the caller's organization
   prefix and returns `{ object_key, upload_url, expires_in_seconds, max_bytes }`.
3. Client `PUT`s the raw image bytes to `upload_url` with the **same**
   `Content-Type` header (the signature includes it). Enforce `max_bytes` and the
   MIME whitelist client-side too; the presigned PUT is not size-limited by S3.
4. Client calls `PATCH /organization` with `{ "logo_object_key": object_key }`.
5. Server verifies the key prefix matches the tenant before storing it.
6. On read, the server returns a presigned GET in `logo_url` (15 min TTL). Render
   `logo_url` directly; do not cache it past expiry. If storage is temporarily
   unavailable, `logo_url` is `null` while `logo_object_key` is still returned.

Replacing a logo: repeat steps 1–4. The previous object is not deleted by the
API yet (see Open questions).

## Error codes

| HTTP | When | `error_stack[].code` |
| ---- | ---- | -------------------- |
| 401 | Missing, expired or revoked session | `unauthorized` |
| 403 | Session lacks the required permission | `forbidden` |
| 404 | Organization or session not found / not owned | `not_found` |
| 409 | Account has no local password (SSO-only) | `conflict` |
| 422 | Validation failed (one entry per field) | `required`, `invalid`, `too_short`, `too_long`, `unchanged`, `out_of_range`, or `validation_error` (envelope-level) |

`422` example:

```json
{
  "error": true,
  "status": 422,
  "error_stack": [
    {
      "field": "asset_tag_padding",
      "code": "out_of_range",
      "message": "asset_tag_padding must be between 1 and 10"
    }
  ],
  "message": "Validation failed"
}
```

## Examples

### GET /me

```http
GET /v1/api/me
Cookie: eat_session=<token>
```

```json
{
  "error": false,
  "status": 200,
  "data": {
    "id": "0f7c…",
    "organization_id": "5b1e…",
    "email": "ada@acme.test",
    "first_name": "Ada",
    "last_name": "Lovelace",
    "name": "Ada Lovelace",
    "phone_number": null,
    "avatar_url": null,
    "status": "active",
    "preferred_locale": "en",
    "auth_provider": "local",
    "employee_id": "EMP-001",
    "job_title": "CTO",
    "roles": ["admin"],
    "role": "admin",
    "email_verified_at": "2026-01-02T10:00:00Z",
    "last_login_at": "2026-02-01T09:12:00Z",
    "created_at": "2026-01-02T09:00:00Z",
    "updated_at": "2026-02-01T09:12:00Z"
  },
  "message": "success"
}
```

### PATCH /me

```json
{ "first_name": "  Grace ", "last_name": "Hopper", "preferred_locale": "fr" }
```

Returns the updated `UserProfile`. Sending `email`, `role`, `status` or
`organization_id` has no effect (unknown keys are ignored).

### POST /me/password

```json
{ "current_password": "Passw0rd!", "new_password": "NewPassw0rd!" }
```

```json
{
  "error": false,
  "status": 200,
  "data": { "revoked_sessions": 2 },
  "message": "Password updated; other sessions signed out"
}
```

### GET /me/sessions

```json
{
  "error": false,
  "status": 200,
  "data": [
    {
      "id": "81f0…",
      "ip_address": "203.0.113.9",
      "user_agent": "Mozilla/5.0 …",
      "created_at": "2026-02-01T09:12:00Z",
      "last_seen_at": "2026-02-01T10:41:00Z",
      "expires_at": "2026-02-08T09:12:00Z",
      "current": true,
      "is_current": true
    }
  ],
  "message": "success"
}
```

The list is **not paginated**; it contains every live (unrevoked, unexpired)
session for the caller.

### DELETE /me/sessions/{id}

`200` with `{ "data": {}, "message": "Session revoked" }`; `404` if the id is
not one of the caller's live sessions. Revoking the current session also clears
the `eat_session` cookie.

### DELETE /me/sessions

```json
{ "error": false, "status": 200, "data": { "revoked": 2 }, "message": "Other sessions revoked" }
```

### GET /organization

```json
{
  "error": false,
  "status": 200,
  "data": {
    "id": "5b1e…",
    "name": "Acme",
    "slug": "acme",
    "default_locale": "en",
    "logo_object_key": null,
    "logo_url": null,
    "settings": {
      "asset_tag_prefix": "EAT-",
      "asset_tag_padding": 4,
      "warranty_alert_days": 30,
      "license_alert_days": 30,
      "currency": "USD",
      "require_handover_signature": false
    },
    "created_at": "2026-01-01T00:00:00Z",
    "updated_at": "2026-02-01T10:00:00Z"
  },
  "message": "success"
}
```

### PATCH /organization

```json
{
  "name": "Acme HQ",
  "default_locale": "fr",
  "settings": {
    "asset_tag_prefix": "ACME-",
    "asset_tag_padding": 6,
    "currency": "eur"
  }
}
```

Only supplied keys change; `warranty_alert_days`, `license_alert_days` and
`require_handover_signature` keep their stored values. Currency is normalized to
uppercase (`"eur"` → `"EUR"`).

### POST /organization/logo/upload-url

```json
{ "file_name": "Logo Final.PNG", "content_type": "image/png" }
```

```json
{
  "error": false,
  "status": 200,
  "data": {
    "object_key": "5b1e…/logo/5b1e…/a2c4-Logo_Final.png",
    "upload_url": "http://garage:3900/eat-assets/5b1e…/logo/…?X-Amz-Signature=…",
    "expires_in_seconds": 900,
    "max_bytes": 2097152
  },
  "message": "success"
}
```

## Field constraints

| Field | Rule |
| ----- | ---- |
| `first_name` / `last_name` | trimmed, 1–100 characters, non-empty |
| `phone_number` | ≤ 40 characters |
| `avatar_url` | `http://` or `https://`, ≤ 1000 characters |
| `preferred_locale` | `en`, `fr`, `ar` |
| organization `name` | trimmed, 1–120 characters |
| `default_locale` | `en`, `fr`, `ar` |
| `asset_tag_prefix` | trimmed, 1–10 characters, no whitespace |
| `asset_tag_padding` | integer 1–10 |
| `warranty_alert_days` / `license_alert_days` | integer 0–3650 |
| `currency` | 3 ASCII letters, normalized to uppercase (shape check only, not a full ISO 4217 lookup) |
| `require_handover_signature` | boolean |
| `logo_object_key` | non-empty, ≤ 500 characters, must start with `${organization_id}/` |
| logo `file_name` | 1–180 characters (sanitized to `[A-Za-z0-9._-]` in the key) |
| logo `content_type` | `image/png`, `image/jpeg`, `image/webp` |
| logo size | ≤ 2 MiB (2 097 152 bytes) |
| `new_password` | ≥ 8 characters, must differ from `current_password` |

## Migration / DB notes

Migration `20260101000009_settings_organization.sql`:

- `organizations.default_locale TEXT NOT NULL DEFAULT 'en'`
  `CHECK (default_locale IN ('en','fr','ar'))`.
- `organizations.logo_object_key TEXT NULL` — Garage S3 key.
- `organizations.settings JSONB NOT NULL DEFAULT '{}'::jsonb` — typed in the API,
  merged on partial updates (`settings || patch`). Rows with missing keys fall
  back to the defaults above.
- `sessions.last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now()` — existing sessions
  get the migration timestamp.

API/FE notes:

- All timestamps are RFC 3339 / ISO 8601 UTC strings, e.g.
  `2026-02-01T09:12:00Z`. Render in the user's timezone client-side.
- `revoked_at` is not exposed; revocation is immediate for the next request.
- `settings` is always returned fully populated (defaults applied), so the FE can
  bind a complete form without null checks.

## Open questions / TODOs for FE

- **Email change** is not supported in M5; it needs a verification flow
  (separate milestone). Do not render an editable email field.
- **Avatar upload** reuses `avatar_url` only (external URL). A presigned-upload
  flow for avatars mirrors the logo one but is not built yet.
- `logo_url` is presigned for ~15 minutes; refresh the organization on demand
  rather than caching the URL long-term.
- Old logo objects are not deleted when replaced; if storage growth matters,
  request a delete-old-key behaviour.
- `PATCH /me` ignores unknown fields (e.g. `email`) instead of returning `422`.
  Disable those inputs in the UI rather than relying on server rejection.
- Currency validation is a shape check only; the FE should offer a curated
  list of ISO 4217 codes.
- Sessions list is unpaginated; if a tenant has many devices, a cap/pagination
  may be added later.
