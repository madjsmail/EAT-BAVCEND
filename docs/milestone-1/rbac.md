# RBAC

Permissions are `resource.action` codes seeded by
`migrations/20260101000002_seed_rbac.sql`. Roles are global system roles; a user
may hold several and receives the union of their permissions.

## Matrix

| Permission | Admin | Manager | Technician | Viewer |
| --- | :-: | :-: | :-: | :-: |
| `organizations.read` | ✅ | ✅ | ✅ | ✅ |
| `organizations.update` | ✅ | — | — | — |
| `users.read` | ✅ | ✅ | ✅ | ✅ |
| `users.update` | ✅ | ✅ | — | — |
| `users.suspend` | ✅ | — | — | — |
| `invitations.read` | ✅ | ✅ | — | — |
| `invitations.create` | ✅ | ✅ | — | — |
| `invitations.revoke` | ✅ | ✅ | — | — |
| `roles.read` | ✅ | ✅ | — | — |
| `roles.assign` | ✅ | ✅ | — | — |
| `sessions.read` | ✅ | ✅ | — | — |
| `sessions.revoke` | ✅ | ✅ | — | — |
| `audit.read` | ✅ | ✅ | — | — |

Totals: Admin 13/13, Manager 11/13, Technician 2/13, Viewer 2/13.

## Intent per role

| Role | Purpose |
| --- | --- |
| Admin | Full control of the tenant: people, roles, sessions, audit. |
| Manager | Day-to-day people operations: onboard, update, offboard. No suspension or org settings. |
| Technician | Field staff: can see colleagues and the organization, nothing else. |
| Viewer | Read-only stakeholder. |

## Enforcement

Handlers call `rbac::require(&ctx, "permission")?`, which returns
`403 forbidden` with `error_stack[0].message = "Missing permission: <code>"`.
Enforced today:

| Endpoint | Permission |
| --- | --- |
| `POST /invitations` | `invitations.create` |
| `GET /invitations` | `invitations.read` |
| `DELETE /invitations/{id}` | `invitations.revoke` |
| `PATCH /users/{id}/status` | `users.suspend` |

Session endpoints intentionally require only authentication: a user manages
**their own** sessions without a permission. `sessions.read` / `sessions.revoke`
are reserved for a future admin-wide session view (e.g. "revoke all sessions of
user X"), which is not part of Milestone 1.

`organizations.read` is implicit: `GET /auth/me` returns the caller's
organization id, and every query is already scoped to it.

## Changing roles

Assignments live in `user_roles` (`organization_id`, `user_id`, `role_id`,
`assigned_by`). Adding a token-less admin action later means: validate the role
is visible to the tenant (`role::find_for_org`), then
`role::assign(&mut tx, org, user, role, assigned_by)` plus an audit entry.

## Adding a permission

1. Insert the row in a migration (`INSERT INTO permissions (code, description)`).
2. Grant it to roles in the same migration.
3. Guard the handler with `rbac::require`.
4. Update this matrix and `docs/milestone-1/database-schema.md`.

Codes are immutable once shipped; handler checks reference the string literal.
