# Local setup

## Prerequisites

- Rust 1.75+ (`cargo`)
- PostgreSQL 17 (Docker Compose provided)
- `psql` (optional, for inspecting data)

## 1. Environment

Copy `.env.example` to `.env` and adjust:

```env
APP_NAME=rust-api
APP_URL=127.0.0.1
APP_PORT=8000

DATABASE_URL=postgres://appuser:change_me_postgres_pass@localhost:5432/appdb
POSTGRES_USER=appuser
POSTGRES_PASSWORD=change_me_postgres_pass
POSTGRES_DB=appdb

# Invite links are built from this host (defaults to http://APP_URL:APP_PORT)
# INVITE_BASE_URL=https://app.example.com

# Session lifetime in hours (default 168, clamped 1..720)
# SESSION_TTL_HOURS=168
```

## 2. Database

```bash
docker compose up -d postgres
```

Migrations run automatically on startup (`sqlx::migrate!("./migrations")`), or
manually:

```bash
sqlx migrate run          # or: cargo run -- bootstrap --help-style args below
```

Tables created: `organizations`, `users`, `invitations`, `sessions`, `roles`,
`permissions`, `role_permissions`, `user_roles`, `audit_logs`.
The second migration seeds permissions and the four system roles.

## 3. Bootstrap the first organization + admin

There is no public signup. Create the tenant and its first Admin from the CLI:

```bash
cargo run -- bootstrap \
  --org-name "Acme Test" \
  --org-slug acme \
  --email admin@acme.test \
  --password 'Passw0rd!' \
  --first-name Ada \
  --last-name Lovelace
```

Output:

```text
organization_id=4cd85f8a-91da-46d5-bbfb-46a01b3f998e
organization_slug=acme
admin_email=admin@acme.test
```

Optional flag: `--phone-number "+213555000000"`. The command is transactional; it
refuses a duplicate slug or email.

## 4. Run

```bash
cargo run
# http://127.0.0.1:8000/v1/api/health
```

## 5. Walk through the flow

```bash
API=http://127.0.0.1:8000/v1/api

# Log in (cookie jar)
curl -s -c jar.txt -X POST $API/auth/login \
  -H 'content-type: application/json' \
  -d '{"organization_slug":"acme","email":"admin@acme.test","password":"Passw0rd!"}'

# Invite a user (grab role id first)
ROLE=$(psql "$DATABASE_URL" -tAc "SELECT id FROM roles WHERE code='viewer'")
curl -s -b jar.txt -X POST $API/invitations \
  -H 'content-type: application/json' \
  -d "{\"email\":\"grace@acme.test\",\"role_id\":\"$ROLE\"}"
# -> data.invite_link = http://127.0.0.1:8000/invite/<token>

# Public preview + acceptance
curl -s $API/invite/<token>
curl -s -X POST $API/invitations/accept \
  -H 'content-type: application/json' \
  -d '{"token":"<token>","password":"Str0ngPass!","first_name":"Grace","last_name":"Hopper"}'

# Invitee logs in
curl -s -c grace.txt -X POST $API/auth/login \
  -H 'content-type: application/json' \
  -d '{"organization_slug":"acme","email":"grace@acme.test","password":"Str0ngPass!"}'

# Sessions
curl -s -b grace.txt $API/sessions
curl -s -b grace.txt -X DELETE $API/sessions/<id>
curl -s -b grace.txt -X DELETE $API/sessions
```

## 6. Tests

```bash
cargo test
```

- 5 unit tests: envelope shape, password hashing, token hashing.
- 5 integration tests in `tests/milestone1.rs`, each on an isolated database
  created and dropped by `#[sqlx::test]` (migrations applied automatically).

Integration tests need `DATABASE_URL` with permission to `CREATE DATABASE`:

```bash
DATABASE_URL=postgres://appuser:change_me_postgres_pass@localhost:5432/appdb cargo test
```

What they prove:

| Test | Proves |
| --- | --- |
| `login_logout_and_session_listing` | login, wrong password, cookie, session list, logout, revoke-all, audit |
| `invite_is_single_use_and_assigns_role` | invite link, preview, acceptance, role assignment, replay → 409, viewer → 403 |
| `expired_invite_is_rejected` | expiry transition + rejection |
| `tenant_isolation_blocks_and_audits_cross_tenant_access` | cross-tenant 404, audit entry, per-tenant sessions, tenant-scoped login, self-suspension guard |
| `suspended_user_sessions_are_revoked` | suspension revokes live sessions immediately |

## 7. Reset

```bash
make clean-db            # DROP SCHEMA public CASCADE, then re-run bootstrap
# or
psql "$DATABASE_URL" -c 'DROP SCHEMA public CASCADE; CREATE SCHEMA public;'
cargo run -- bootstrap --org-name "Acme Test" --org-slug acme \
  --email admin@acme.test --password 'Passw0rd!' \
  --first-name Ada --last-name Lovelace
```

## Troubleshooting

| Symptom | Cause |
| --- | --- |
| `Migrations failed` | `DATABASE_URL` unreachable or schema owned by another role |
| Login always 401 | `status` not `active`, or wrong `organization_slug` |
| Cookie not stored by browser | serving over HTTP with a non-local `APP_URL`, so `Secure` is set |
| Invite link points at `127.0.0.1` in production | set `INVITE_BASE_URL` |
