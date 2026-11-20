# API

Base path: `/v1/api`. All responses use the project envelope:

```json
{ "error": false, "status": 200, "data": {}, "message": "success" }
```

Errors:

```json
{
  "error": true,
  "status": 403,
  "error_stack": [{ "field": null, "code": "forbidden", "message": "Missing permission: invitations.create" }],
  "message": "Missing permission: invitations.create"
}
```

## Endpoint summary

| Method | Path | Auth | Permission |
| --- | --- | --- | --- |
| GET | `/health` | no | — |
| POST | `/auth/login` | no | — |
| POST | `/auth/logout` | cookie | — |
| GET | `/auth/me` | cookie | — |
| GET | `/invite/{token}` | no | — (token is the credential) |
| POST | `/invitations/accept` | no | — (token is the credential) |
| POST | `/invitations` | cookie | `invitations.create` |
| GET | `/invitations` | cookie | `invitations.read` |
| DELETE | `/invitations/{id}` | cookie | `invitations.revoke` |
| GET | `/sessions` | cookie | — (own sessions) |
| DELETE | `/sessions/{id}` | cookie | — (own sessions) |
| DELETE | `/sessions` | cookie | — (own sessions) |
| PATCH | `/users/{id}/status` | cookie | `users.suspend` |

Authentication is a cookie named `eat_session`:
`HttpOnly; SameSite=Lax; Path=/`, plus `Secure` outside localhost.

## POST /auth/login

Login is tenant-aware because emails are unique **per organization**.

```json
{ "organization_slug": "acme", "email": "admin@acme.test", "password": "Passw0rd!" }
```

`200` – sets the cookie:

```json
{
  "error": false,
  "status": 200,
  "data": {
    "user": {
      "id": "…",
      "organization_id": "…",
      "email": "admin@acme.test",
      "first_name": "Ada",
      "last_name": "Lovelace",
      "phone_number": null,
      "avatar_url": null,
      "status": "active",
      "preferred_locale": "en",
      "auth_provider": "local",
      "last_login_at": null
    },
    "permissions": ["audit.read", "invitations.create", "…"],
    "session_id": "…",
    "expires_at": "2026-11-27T14:26:24Z"
  },
  "message": "success"
}
```

`401 unauthorized` for unknown organization, unknown user, wrong password or
inactive account (deliberately indistinguishable). Attempts are audited as
`auth.login_failed`.

## POST /auth/logout

Revokes the current session and clears the cookie. `200`, body
`{ "data": {}, "message": "Logged out" }`. Idempotent from the client's view.

## GET /auth/me

Returns `{ user, permissions, organization_id, session_id }` for the current
session. `401` without a valid cookie.

## GET /invite/{token}

Public acceptance-page lookup (raw token from the link).

```json
{
  "error": false,
  "status": 200,
  "data": {
    "organization_name": "Acme Test",
    "email": "new.user@acme.test",
    "role_name": "Viewer",
    "expires_at": "2026-11-23T14:26:24Z",
    "status": "pending"
  }
}
```

`404 not_found` for unknown token. A pending token past `expires_at` is
transitioned to `expired` and returned with `"status": "expired"`.

## POST /invitations/accept

```json
{
  "token": "<raw token from the link>",
  "password": "Str0ngPass!",
  "first_name": "Grace",
  "last_name": "Hopper",
  "phone_number": "+213555000111",
  "avatar_url": "https://cdn.test/grace.png",
  "preferred_locale": "fr"
}
```

`200` with the activated `UserProfile` (`status: "active"`). Does **not** log the
user in; they log in afterwards.

| Status | When |
| --- | --- |
| `400 invalid` | password < 8 chars, missing names, expired token (`code: expired`) |
| `404 not_found` | unknown token |
| `409 conflict` | token already accepted/revoked, or a user with that email already exists |

The invite row is locked `FOR UPDATE`; two concurrent accepts cannot both succeed.

## POST /invitations

Requires `invitations.create`.

```json
{ "email": "New.User@Acme.test", "phone_number": "+213555000222", "role_id": "…", "expires_in_hours": 24 }
```

`200`:

```json
{
  "error": false,
  "status": 200,
  "data": {
    "id": "…",
    "email": "new.user@acme.test",
    "phone_number": "+213555000222",
    "role_code": "viewer",
    "status": "pending",
    "expires_at": "2026-11-21T14:26:24Z",
    "invite_link": "http://127.0.0.1:8000/invite/9f1c…"
  },
  "message": "success"
}
```

- Email is normalised to lower-case; the role must be a system role or the
  tenant's own role.
- `expires_in_hours` defaults to 72 and is clamped to 1–720.
- `409 conflict` when the email already has an active user or a pending invite.
- `400 invalid` for a malformed email; `404 not_found` for an unknown role.
- `403 forbidden` without the permission.

The raw token appears **once**, inside `invite_link`. Only its SHA-256 hash is
stored. Share the link over WhatsApp/email; `INVITE_BASE_URL` controls the host.

## GET /invitations

Requires `invitations.read`. Returns the tenant's invitations, newest first.
Hashes and tokens are never returned.

## DELETE /invitations/{id}

Requires `invitations.revoke`. Only `pending` invitations are revoked; otherwise
`404 not_found`. Audited as `invitation.revoked`.

## GET /sessions

Returns the caller's live sessions, newest first:

```json
{
  "data": [
    {
      "id": "…",
      "ip_address": "203.0.113.7",
      "user_agent": "Mozilla/5.0",
      "created_at": "2026-11-20T14:26:24Z",
      "expires_at": "2026-11-27T14:26:24Z",
      "current": true
    }
  ]
}
```

Revoked and expired sessions are excluded.

## DELETE /sessions/{id}

Revokes one of the caller's sessions. Revoking the current session also clears
the cookie. `404 not_found` when the id is not the caller's — including when it
belongs to another tenant, which is additionally audited as
`tenant.cross_access_denied`.

## DELETE /sessions

Forced logout: revokes **all** of the caller's sessions, clears the cookie and
returns `{ "revoked": <n> }`. Audited as `session.revoked_all`.

## PATCH /users/{id}/status

Requires `users.suspend`.

```json
{ "status": "suspended" }
```

`active` or `suspended`. Suspension revokes every session of that user in the
same transaction. `409` when targeting yourself; `404` for another tenant's user
or an unknown id. Audited as `user.status_changed`.

## Invite flow

```mermaid
sequenceDiagram
    autonumber
    actor Admin
    participant API
    participant DB
    actor Invitee
    participant Page as Invite page

    Admin->>API: POST /invitations {email, role_id}
    API->>API: validate + RBAC (invitations.create)
    API->>DB: INSERT invitations (token_hash, expires_at)
    API->>DB: INSERT audit_logs (invitation.created)
    API-->>Admin: 200 {invite_link}
    Admin-->>Invitee: share link (WhatsApp / email)

    Invitee->>Page: open /invite/{token}
    Page->>API: GET /invite/{token}
    API->>DB: SELECT invitations WHERE token_hash = sha256(token)
    API-->>Page: org name, prefilled email, role, expiry

    Invitee->>Page: submit profile + password
    Page->>API: POST /invitations/accept
    API->>DB: BEGIN; SELECT … FOR UPDATE
    alt token pending and not expired
        API->>DB: create/activate user, assign role, accepted_at
        API->>DB: INSERT audit_logs (invitation.accepted)
        API->>DB: COMMIT
        API-->>Page: 200 UserProfile (active)
    else already used / expired
        API->>DB: ROLLBACK (or mark expired)
        API-->>Page: 409 / 400
    end

    Invitee->>API: POST /auth/login
    API->>DB: INSERT sessions + audit_logs (auth.login)
    API-->>Invitee: Set-Cookie eat_session + permissions
```

## Error codes

| HTTP | `code` | Meaning |
| --- | --- | --- |
| 400 | `invalid` | Validation failure; see `error_stack[].field` |
| 400 | `expired` | Invitation expired |
| 401 | `unauthorized` | Missing/invalid/revoked/expired session |
| 403 | `forbidden` | Missing permission |
| 404 | `not_found` | Resource absent **or** owned by another tenant |
| 409 | `conflict` | Duplicate email/pending invite, replayed token, self-suspension |
| 500 | `db_error` / `internal_error` | Logged server-side, details not leaked |
