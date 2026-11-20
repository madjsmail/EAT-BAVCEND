# OAuth extension (design only — not implemented)

This document describes how Google and Microsoft sign-in will attach to the
existing auth core. No OAuth code ships in Milestone 1.

## What already fits

| Piece | Reusable as-is |
| --- | --- |
| `users.auth_provider` | `local` today; `google` / `microsoft` later |
| `users.password_hash` (nullable) | federated users simply have `NULL` |
| `sessions` | issued identically after any authentication method |
| RBAC (`user_roles` → `role_permissions`) | independent of how identity was proven |
| Audit | add `auth.login` with `metadata.provider` |
| Invite-only policy | OAuth **links** an identity; it must not self-register |

## Proposed tables (added in the OAuth milestone)

```sql
CREATE TABLE oauth_identities (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    organization_id  UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id          UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider         TEXT NOT NULL CHECK (provider IN ('google', 'microsoft')),
    subject          TEXT NOT NULL,          -- provider's stable user id (sub)
    email            TEXT NOT NULL,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at     TIMESTAMPTZ,
    UNIQUE (provider, subject),
    UNIQUE (organization_id, user_id, provider)
);
```

Why a separate table: one user may link several providers, and `users` stays free
of provider-specific columns.

`organizations` gains optional policy columns:

```sql
ALTER TABLE organizations
    ADD COLUMN allowed_email_domains TEXT[],          -- e.g. {acme.com}
    ADD COLUMN oauth_auto_provision BOOLEAN NOT NULL DEFAULT FALSE;
```

## Flow

```mermaid
sequenceDiagram
    autonumber
    actor User
    participant FE as Frontend
    participant API
    participant Provider as Google / Microsoft
    participant DB

    User->>FE: "Continue with Google"
    FE->>API: GET /auth/oauth/{provider}/start?org_slug=acme&redirect_uri=…
    API->>API: sign state (HMAC, 10 min TTL) + PKCE verifier
    API-->>FE: 302 to provider authorize URL (state, PKCE challenge)
    Provider-->>User: consent screen
    Provider->>API: GET /auth/oauth/{provider}/callback?code=…&state=…
    API->>API: verify state + PKCE
    API->>Provider: POST token endpoint (code exchange)
    Provider-->>API: id_token + access_token
    API->>API: verify id_token signature, iss, aud, exp, nonce
    API->>DB: find oauth_identities (provider, sub)
    alt identity found and user active
        API->>DB: INSERT sessions + audit (auth.login, provider)
        API-->>FE: 302 redirect_uri + Set-Cookie
    else identity not found
        API->>DB: domain allowed and auto-provision on?
        alt provision allowed
            API->>DB: create user (auth_provider, no password) + assign default role
            API->>DB: INSERT oauth_identities
        else
            API-->>FE: 403 "no account; ask an admin for an invite"
        end
    end
```

## Design rules

1. **No implicit self-registration.** Unknown identities are rejected unless the
   organization has explicitly enabled `oauth_auto_provision` for their domain.
2. **Tenant is chosen before the redirect.** `org_slug` is part of the signed
   `state`; the callback never infers the tenant from the email.
3. **`state` is signed and single-use**, with a nonce tied to the PKCE verifier.
4. **ID tokens are verified** against the provider JWKS (cached), checking
   signature, `iss`, `aud`, `exp`, `nonce`.
5. **Linking requires an authenticated session.** Adding a provider to an
   existing account is `POST /auth/oauth/{provider}/link` behind a normal session;
   an OAuth identity alone can never take over an email-based account.
6. **Password and OAuth can coexist**: `password_hash` stays nullable, so a user
   with a local password can link Google later.
7. **Suspension wins.** The session layer already requires `users.status = 'active'`;
   OAuth issues the same session rows, so suspension revokes access immediately.
8. **Secrets** live in the environment (`OAUTH_GOOGLE_CLIENT_SECRET`,
   `OAUTH_MICROSOFT_CLIENT_SECRET`), never in the database.
9. **Audit** every attempt: `auth.oauth_started`, `auth.login` with
   `metadata.provider`, and `auth.oauth_rejected` with a reason.

## Code touch points when implementing

| File | Change |
| --- | --- |
| `migrations/` | new migration for `oauth_identities` + org policy columns |
| `src/config/parameter.rs` | provider client ids/secrets, redirect base URL |
| `src/handler/oauth.rs` | start/callback/link handlers |
| `src/repository/mod.rs` | `oauth_identity::{find, insert}`, user provisioning |
| `src/service/mod.rs` | state signing, PKCE, id-token verification helpers |
| `src/routes/root.rs` | three public routes + one authenticated link route |
| tests | state tampering, unknown identity rejection, domain policy, suspension |

No existing table shape or endpoint must change: `auth_provider` and
`password_hash` were designed for this from the start.
