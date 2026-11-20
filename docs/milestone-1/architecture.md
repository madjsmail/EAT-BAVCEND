# Architecture

## 1. Tenant isolation: application middleware, not RLS

**Decision:** the tenant is resolved from the session row by an Axum extractor and
every repository query takes `organization_id` explicitly. PostgreSQL Row Level
Security is **not** enabled in Milestone 1.

Rationale:

- The connection pool is shared; RLS would require `SET LOCAL app.organization_id`
  inside every transaction, otherwise a pooled connection leaks the previous
  tenant's setting. That is one forgotten `SET` away from a cross-tenant read.
- Explicit `organization_id = $1` in every statement is greppable and reviewable.
- Tests can prove isolation at the API boundary, which is what matters.

Trade-off: protection depends on developer discipline. Mitigations:

1. All SQL lives in `src/repository/mod.rs`; the rule is one file.
2. Every tenant-scoped query binds `organization_id` first.
3. Cross-tenant probes are detected where an ID is user-supplied and written to
   `audit_logs` as `tenant.cross_access_denied`.

RLS remains a valid hardening step later (see `decisions.md`, ADR-004).

### Cross-tenant detection

A query scoped to the caller's organization simply returns no rows, which looks
like a 404. For `DELETE /sessions/{id}` the handler additionally probes
`SELECT organization_id FROM sessions WHERE id = $1`. If the row exists under a
different organization, an audit entry is written before returning 404. Other
resources follow the same pattern when their endpoints are added.

## 2. Request lifecycle

```text
HTTP request
  └─ Router (/v1/api)
      └─ AuthContext extractor        (only on authenticated routes)
          ├─ read `eat_session` cookie
          ├─ SHA-256 the token
          ├─ load live session (not revoked, not expired) + active user
          └─ load permission codes from user_roles -> role_permissions
      └─ handler
          ├─ validate payload
          ├─ rbac::require(&ctx, "permission")  // 403 on miss
          ├─ repository call with ctx.organization_id()
          ├─ audit_logs insert
          └─ ApiResponse envelope
```

The extractor is the only place that knows about cookies. Handlers never see the
token, never accept `organization_id` from the body or query string, and cannot
construct an `AuthContext` themselves.

## 3. Session strategy

- Opaque 256-bit random token, hex encoded, returned only in `Set-Cookie`.
- Only `SHA-256(token)` is stored (`sessions.token_hash`), so a database dump
  cannot be replayed as a session.
- Cookie: `HttpOnly; SameSite=Lax; Path=/`, `Secure` unless `APP_URL` is
  `localhost`/`127.0.0.1`/`0.0.0.0` (local HTTP development).
- TTL: `SESSION_TTL_HOURS`, default 168 h, clamped 1 h – 720 h.
- Validity is evaluated per request: `revoked_at IS NULL AND expires_at > now()`.
  Logout, "revoke all" and suspension take effect immediately.
- Sessions are tenant-scoped: a session row carries `organization_id`, and the
  user must still belong to that organization and be `active`.

Why stateful: instant revocation, per-device listing, and no token-revocation
list to invent. Cost: one indexed lookup per request, which is acceptable.

## 4. RBAC

- `permissions` is a global catalogue of `resource.action` codes.
- `roles` are system rows (`organization_id IS NULL`, `is_system = true`) shared by
  every tenant; the column allows tenant-specific roles later.
- `role_permissions` maps roles to permissions.
- `user_roles` is tenant-scoped and links a user to a role.

The extractor resolves the user's permission codes once per request, so guards
cost nothing extra. `rbac::require(&ctx, "invitations.create")?` returns 403 with
`error_stack[0].code = "forbidden"`.

A user can hold several roles; permissions are the union.

## 5. Token & password storage

| Secret | Algorithm | Stored |
| --- | --- | --- |
| Password | Argon2id (PHC string, random salt) | `users.password_hash` |
| Session token | SHA-256 | `sessions.token_hash` |
| Invite token | SHA-256 | `invitations.token_hash` |

Invite and session tokens are high-entropy random values, so a fast hash is
sufficient and keeps lookups cheap. Passwords are low-entropy and use Argon2id.

## 6. Module map

```text
config/     env + pool (Database::init, Database::from_pool)
middleware/ AuthContext extractor, permission guard, cookie/ip helpers
repository/ SQL, grouped by aggregate, every query org-scoped
service/    Argon2, token generation/hashing, cookie formatting
handler/    thin Axum handlers, validation, audit
routes/     router factory `app(state)` (shared with tests)
cli.rs      `bootstrap` subcommand
```

`routes::root::app(Arc<Database>)` returns a `Router`, and the server wraps it in
`into_make_service()`. Tests build the same router against an isolated pool.

## 7. Extension points

- `users.auth_provider` distinguishes `local` from future `google`/`microsoft`.
- `users.password_hash` is nullable: federated accounts have no local password.
- Session issuance, RBAC resolution and audit writes are independent of how a
  user was authenticated, so a future OAuth callback only has to prove identity
  and then reuse `session::create` + `role::permissions_for_user`.
- See [oauth-extension.md](./oauth-extension.md).
