# Architecture decisions (ADR)

## ADR-001 — Invitation and session tokens are stored as SHA-256 hashes

**Context.** A database dump must not let an attacker mint sessions or accept
invites.

**Decision.** Tokens are 256-bit random values (`OsRng`), returned once, and only
`sha256(token)` (hex) is persisted in `token_hash` (unique). Lookup is by hash, so
the raw value is never needed again.

**Alternatives.** Store the token in plaintext (simple, one DB leak = full
account takeover); HMAC with a server key (equivalent here, extra key management).

**Consequences.** A token cannot be re-displayed after creation; the invite link
is returned exactly once and must be re-issued (revoke + recreate) if lost. A
fast hash is acceptable because the input is high entropy, unlike passwords which
use Argon2id.

---

## ADR-002 — Invite links are static URLs carrying the raw token

**Context.** Invites are delivered out-of-band (WhatsApp, email, SMS) and the
product has no email provider yet.

**Decision.** `POST /invitations` returns `invite_link = {INVITE_BASE_URL}/invite/{token}`.
The frontend route is public and calls `GET /invite/{token}` to render the
acceptance page, then `POST /invitations/accept`.

**Alternatives.** Email the token directly (needs SMTP/provider + templates, out
of scope); one-time numeric codes (weaker, needs rate limiting).

**Consequences.** The link is a bearer credential until it expires. Defaults:
72 h TTL, single pending invite per email per tenant (DB partial unique index),
revocable by an admin. Anything that logs URLs (proxies, analytics) must be
treated as sensitive. Delivery is explicitly the caller's responsibility.

---

## ADR-003 — Tenant isolation in middleware + repositories, not RLS

**Context.** Every SaaS query must be scoped to one organization.

**Decision.** An Axum extractor (`AuthContext`) resolves the session and exposes
`organization_id()`; every repository function takes it and binds it. No query
accepts a tenant from the request body or query string.

**Alternatives.**
- PostgreSQL RLS with `SET LOCAL app.organization_id`. Robust at the DB layer,
  but requires every pooled transaction to set it; one missed `SET` silently
  leaks the previous tenant's context.
- Schema-per-tenant: migration and connection explosion.

**Consequences.** Protection is a code convention. Mitigations: single repository
file, review checklist, integration tests, and cross-tenant probes logged to
`audit_logs`. RLS can be layered on later without changing the API.

---

## ADR-004 — Sessions are stateful rows, not JWTs

**Context.** Requirements include live session listing, per-session revocation and
forced logout after suspension.

**Decision.** Opaque token in an HTTP-only cookie; the `sessions` table is the
source of truth (`revoked_at`, `expires_at`, `ip_address`, `user_agent`).

**Alternatives.** JWT access tokens (no revocation without a denylist, which
reintroduces state); short-lived JWT + refresh rotation (two mechanisms, more code).

**Consequences.** One indexed lookup per request; instant revocation and an
auditable device list. The cookie must be `SameSite=Lax`/CSRF-protected if the
frontend ever moves cross-site.

---

## ADR-005 — Emails are unique per organization, so login requires the org slug

**Context.** The same person may belong to two tenants with the same email
(the schema requires per-org uniqueness).

**Decision.** `users` has `UNIQUE (organization_id, email)` with lower-case
normalisation. Login payload is `{ organization_slug, email, password }`.

**Alternatives.** Global email uniqueness (blocks legitimate multi-tenant users);
one login screen that emails a tenant list (account enumeration).

**Consequences.** The login screen collects the workspace, like Slack/Notion. A
future "find my workspaces" endpoint should be rate-limited and must not confirm
which workspaces exist.

---

## ADR-006 — Roles are global system rows with a nullable tenant column

**Context.** Four roles ship with the product; tenants do not create roles in
Milestone 1.

**Decision.** `roles.organization_id IS NULL` means "system role shared by all
tenants"; `is_system` marks it. `role::find_for_org` accepts system rows plus the
caller's own.

**Alternatives.** Seed four roles per organization at bootstrap (works, but every
new org must remember to seed, and role ids differ per tenant); hard-code roles in
Rust (loses the permission join).

**Consequences.** Custom per-tenant roles are additive: insert with
`organization_id = $tenant`. `roles.code` is globally unique for now; if tenant
roles need to reuse a code, that unique constraint becomes
`(organization_id, code)`.

---

## ADR-007 — Invitation acceptance is transactional and idempotency is enforced by the row lock

**Context.** Users double-click, refresh, or replay links; concurrent accepts must
not create two users.

**Decision.** `SELECT … FOR UPDATE` on the invitation row inside a transaction,
re-check `status = 'pending'` and `expires_at > now()`, then create/activate the
user, assign the role, mark `accepted_at`, and write the audit row before commit.
A second attempt sees `status = 'accepted'` and returns `409 conflict`.

**Alternatives.** Unique constraint on `users.email` alone (races still produce
partial writes); an in-memory lock (does not survive restarts).

**Consequences.** Acceptance is at-most-once. Expired tokens are transitioned to
`status = 'expired'` and returned as `400`.

---

## ADR-008 — Audit rows are written in the same transaction as the state change

**Context.** An audit trail that can disagree with the data is worse than none.

**Decision.** Acceptance, role assignment, suspension and session revocation use
`audit::insert_tx` inside the transaction. Read-only events (logins) use the pool
directly.

**Consequences.** A failed audit insert fails the operation. `audit_logs` is
append-only; only `INSERT` statements exist.

---

## ADR-009 — No public signup, ever

**Context.** "Invite-only auth" is a product requirement, not a preference.

**Decision.** No `/register` or `/signup` route exists. Users are created by the
bootstrap CLI or by invitation acceptance. `users.status` starts as `invited` for
pre-created rows and becomes `active` only through acceptance.

**Consequences.** Onboarding requires an Admin. A future SSO flow may auto-create
users, but only for domains an admin has explicitly authorized.
