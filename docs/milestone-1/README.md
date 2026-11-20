# Milestone 1 — Foundation, Multi-Tenancy & Invite-Only Auth

> Status: implemented, tested (`cargo test`), PR open against `main`.
> Stack note: this repository is the **Rust/Axum** backend (`AGENTS.md` locks the
> stack), so the milestone was built in Rust with `axum`, `sqlx` and PostgreSQL
> instead of Node.js. API shape, cookies, schema and behaviour match the milestone
> spec one-for-one.

## Goals

1. A PostgreSQL schema for organizations, users, invitations, sessions and RBAC.
2. Hard tenant isolation: the organization is always derived from the session,
   never from client input.
3. Invite-only onboarding: no public signup, single-use expiring invite links.
4. Stateful cookie sessions with listing and revocation.
5. An audit trail for every auth, invitation and cross-tenant event.
6. Clean extension points for Google/Microsoft OAuth (design only, no code).

## Scope

| Area | Delivered |
| --- | --- |
| Schema | 9 tables + seed of permissions and the Admin/Manager/Technician/Viewer roles |
| Tenant isolation | Session-derived `AuthContext`, every query filters `organization_id`, cross-tenant probes audited |
| Auth | Bootstrap CLI, stateful login/logout, HTTP-only cookie sessions |
| Invitations | Create (returns shareable link), public preview, single-use acceptance |
| Sessions | List active, revoke one, revoke all, auto-revoke on suspension |
| RBAC | Permission guard middleware/handler helper + seeded permission matrix |
| Audit | Invites, acceptances, logins, failed logins, revocations, cross-tenant denials |
| Tests | 10 tests (5 unit, 5 integration) against an isolated PostgreSQL database per test |

## Out of scope

- OAuth providers (Google/Microsoft) — extension design only.
- Inventory/asset screens and APIs.
- Billing.
- Password reset / password change flows (revocation hook is in place).
- Email/WhatsApp delivery (the API returns the link; delivery is the caller's job).

## Acceptance checklist

- [x] Admin logs in via the bootstrap account.
- [x] Admin invites a user, gets a shareable link, assigns a role.
- [x] Invitee accepts via link, sets profile, logs in.
- [x] Invites are single-use and expire.
- [x] Session list + forced revocation work.
- [x] Integration tests prove strict tenant isolation.
- [x] Audit log records invites, logins, revocations, rejected cross-tenant attempts.
- [x] `auth_provider` column + clean OAuth extension points (no OAuth code).

## Documentation map

| Document | Contents |
| --- | --- |
| [architecture.md](./architecture.md) | Tenant isolation, sessions, RBAC, request lifecycle |
| [database-schema.md](./database-schema.md) | Tables, columns, FKs, indexes, ERD |
| [api.md](./api.md) | Endpoints, payloads, errors, invite sequence |
| [rbac.md](./rbac.md) | Role → permission matrix |
| [setup.md](./setup.md) | Local setup, migrations, bootstrap, tests |
| [decisions.md](./decisions.md) | ADR log |
| [oauth-extension.md](./oauth-extension.md) | Future Google/Microsoft OAuth design |

## Where the code lives

```text
migrations/                                  schema + RBAC seed
src/config/database.rs                       pool, Database::from_pool (tests)
src/middleware/auth.rs                       AuthContext extractor (session -> tenant)
src/middleware/rbac.rs                       require(ctx, "permission")
src/repository/mod.rs                        all SQL, org-scoped
src/service/mod.rs                           Argon2, token hashing, cookie helpers
src/handler/mod.rs                           thin handlers + audit writes
src/routes/root.rs                           router (also used by tests)
src/cli.rs                                   bootstrap command
tests/milestone1.rs                          acceptance + isolation tests
```
