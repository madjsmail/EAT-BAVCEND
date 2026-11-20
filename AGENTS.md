# AGENTS.md — Rust Asset Tracking API (`backend/`)

## 1. Mission & Strict Scope Guardrails
You are working strictly inside the **Rust Backend API** (`backend/`).
- **NEVER** modify frontend files (`web/`), Docker infrastructure, or unrelated modules.
- **ONE VERTICAL SLICE AT A TIME:** Only touch files needed for the single requested feature (e.g., `migrations/` + `src/modules/<feature>/`).
- **NO SPECULATIVE REFACTORING:** Do not rename existing structs, change crate versions in `Cargo.toml`, or rewrite shared utilities unless explicitly instructed.
- **BRANCH FIRST:** Create a feature branch (e.g. `feature/milestone-N-<scope>`) before the first edit of any task; never work directly on `main`.
- **STOP ON 2 COMPILE ERRORS:** If `cargo check` fails twice on the same issue, stop immediately, output the exact `rustc` error, and ask for direction instead of looping.

---

## 2. Locked Tech Stack
- **Language:** Rust (2021 edition)
- **Web Framework:** `axum` (0.7+) + `tokio`
- **Database:** PostgreSQL + `sqlx` (compile-time checked or runtime `sqlx::query_as!` / `sqlx::query!`)
- **Object Storage:** Garage S3-compatible API via `aws-sdk-s3` (force path-style addressing enabled)
- **Auth & Crypto:** Stateful sessions in PostgreSQL (`sessions` table), HTTP-only secure cookies, `argon2` for password hashing, `sha2` for session/invite token hashing.
- **Validation & Serialization:** `serde`, `serde_json`, `validator`, `uuid` (v4/v7), `chrono` (`DateTime<Utc>`).

---

## 3. Canonical Directory Structure
Follow this layout strictly for every domain module:
```text
backend/
├── migrations/                  # Sequential SQLx migrations (YYYYMMDDHHMMSS_name.sql)
└── src/
    ├── main.rs                  # Router assembly & server startup
    ├── config.rs                # Env vars (DATABASE_URL, GARAGE_ENDPOINT, GARAGE_BUCKET, etc.)
    ├── error.rs                 # Unified AppError -> HTTP status + JSON { error, code }
    ├── state.rs                 # AppState { db: PgPool, s3: aws_sdk_s3::Client, config }
    ├── middleware/
    │   ├── auth.rs              # Extracts AuthContext { user_id, organization_id, permissions }
    │   └── rbac.rs              # Permission guard helper: require_permission(&ctx, "assets.create")?
    ├── storage/
    │   └── garage.rs            # Presigned URL & multipart upload wrapper for Garage S3
    └── modules/
        ├── auth/                # Login, logout, active sessions (NO public signup)
        ├── invitations/         # Admin invite creation & token acceptance
        ├── users/               # Users, roles, permissions
        ├── lookups/             # Brands, categories, locations, departments, vendors
        ├── products/            # Device catalog models (Samsung, Huawei, etc.)
        ├── assets/              # Serialized assets, IMEI lookup, QR payload, assignments
        ├── sim_cards/           # SIM registry & linking to assets/users
        ├── accessories/         # Bulk accessories & dual-target assignments
        ├── consumables/         # Stock levels & append-only transactions
        ├── licenses/            # Software licenses & seat allocations
        ├── maintenances/        # Preventive schedules, repair tickets, parts used
        ├── attachments/         # Polymorphic metadata + Garage S3 upload/delete
        └── audit/               # Append-only audit_logs helper & read-only query endpoint
```

Every domain module inside `src/modules/<name>/` MUST contain only:
- `mod.rs` (exports `router() -> Router<AppState>`)
- `dto.rs` (Request/Response structs with `Deserialize`/`Serialize`/`Validate`)
- `repo.rs` (Pure SQLx functions taking `&PgPool` or `&mut Transaction` and `org_id: Uuid`)
- `handlers.rs` (Thin Axum handlers: validate -> check RBAC -> call `repo` -> write `audit_log` -> return JSON)

---

## 4. Non-Negotiable Database & Security Rules
1. **Multi-Tenancy Isolation:**
   - Every SQL query on a tenant-scoped table **MUST** filter by `organization_id = $1` using `auth.organization_id` from the session extractor—**NEVER** trust an `organization_id` sent in a request body or query param.
2. **Invite-Only Auth:**
   - Never create a public `/register` or `/signup` route.
   - Store only SHA-256 hashes (`token_hash`) of invitation tokens and session tokens in PostgreSQL.
3. **JSONB Localization (`i18n`):**
   - Localized fields (`name_i18n`, `description_i18n`) are stored as `serde_json::Value` (`{"en": "...", "fr": "...", "ar": "..."}`). Validate that at least one non-empty locale key exists.
4. **Atomic State Transitions:**
   - Assigning an asset, issuing consumables, allocating license seats, or logging a repair must run inside a `sqlx::Transaction` and insert an `audit_logs` row before committing.
5. **Garage S3 Rules:**
   - Object keys MUST follow: `{organization_id}/{entity_type}/{entity_id}/{uuid}-{sanitized_filename}`.
   - Always configure `force_path_style(true)` on the S3 client for Garage compatibility.

---

## 5. Verification Checklist (Before Finishing Any Task)
1. Run `cargo check` (and `sqlx migrate run` if a migration was added).
2. Confirm zero `.unwrap()` or `.expect()` calls in request handlers or repositories (use `?` with `AppError`).
3. Confirm every new query includes `organization_id`.
4. Keep your final response under 5 bullet points listing only changed files and verification results.


## Database & Schema Rules

- **Schema Map:** `/docs/database_schema_mermaid_erd.md` (table relationships & keys)
- **Schema Details:** `/docs/database_schema_description.md` (column types, constraints & business rules)

### Workflow
1. **Before writing queries or joins:** Read `database_schema_mermaid_erd.md` first to verify table names and foreign keys.
2. **Before writing migrations, validations, or mutations:** Read `database_schema_description.md` to check constraints and defaults.
3. **Zero Drift Rule:** Whenever you create or modify a database migration, you MUST update both `database_schema_mermaid_erd.md` and `database_schema_description.md` in the same task.


---

## 6. GitHub Project Board

All backlog items, bugs, and deferred work go to **EAT**:
https://github.com/users/madjsmail/projects/17

Repository: `madjsmail/EAT-BAVCEND`. Add issues to project `17` and set
Status to **Backlog**. Project and field IDs are in `.opencode/instructions.md`.

### 6.1 GitHub MCP Issue/PR Format (front vs back)

When filing bugs, tasks, or PRs via the GitHub MCP, always tag the affected
area so triage is instant:

- **Label:** apply exactly one of `front` (frontend/SvelteKit) or `back`
  (backend/Rust). Never both; pick the primary affected area.
- **Title prefix:** start the title with the area tag —
  `[front] Bug: ...` or `[back] Bug: ...` (same for tasks/PRs:
  `[front] Task: ...`, `[back] PR: ...`).
- **Body (bug template):**
  ```
  ## Summary
  <one-line what is broken>

  ## Area
  <module/route/component affected, e.g. assets/[id] or modules/assets>

  ## Steps to Reproduce
  1. ...
  2. ...

  ## Expected
  <what should happen>

  ## Actual
  <what happens instead>

  ## Environment
  <browser/OS or endpoint/version, if relevant>
  ```
- Use the labels `front`/`back` only; do not invent new area labels. If a
  change spans both, file two linked issues (one per area) and cross-reference.
