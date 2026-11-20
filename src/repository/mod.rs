// Data access layer. Every tenant-scoped query filters by organization_id.
#![allow(dead_code)]

use chrono::{DateTime, Utc};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::entity::{Invitation, Organization, Role, Session, User};

pub mod organization {
    use super::*;

    pub async fn create(pool: &PgPool, name: &str, slug: &str) -> sqlx::Result<Organization> {
        sqlx::query_as::<_, Organization>(
            "INSERT INTO organizations (name, slug) VALUES ($1, $2) RETURNING *",
        )
        .bind(name)
        .bind(slug)
        .fetch_one(pool)
        .await
    }

    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> sqlx::Result<Option<Organization>> {
        sqlx::query_as::<_, Organization>("SELECT * FROM organizations WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await
    }

    pub async fn find_by_slug(pool: &PgPool, slug: &str) -> sqlx::Result<Option<Organization>> {
        sqlx::query_as::<_, Organization>("SELECT * FROM organizations WHERE slug = $1")
            .bind(slug)
            .fetch_optional(pool)
            .await
    }
}

pub mod user {
    use super::*;

    pub async fn find_by_email(
        pool: &PgPool,
        organization_id: Uuid,
        email: &str,
    ) -> sqlx::Result<Option<User>> {
        sqlx::query_as::<_, User>("SELECT * FROM users WHERE organization_id = $1 AND email = $2 AND deleted_at IS NULL")
            .bind(organization_id)
            .bind(email)
            .fetch_optional(pool)
            .await
    }

    pub async fn find_by_id(
        pool: &PgPool,
        organization_id: Uuid,
        id: Uuid,
    ) -> sqlx::Result<Option<User>> {
        sqlx::query_as::<_, User>("SELECT * FROM users WHERE organization_id = $1 AND id = $2 AND deleted_at IS NULL")
            .bind(organization_id)
            .bind(id)
            .fetch_optional(pool)
            .await
    }

    /// Insert an active user inside a transaction (invite acceptance).
    #[allow(clippy::too_many_arguments)]
    pub async fn insert_active(
        conn: &mut PgConnection,
        organization_id: Uuid,
        email: &str,
        first_name: &str,
        last_name: &str,
        phone_number: Option<&str>,
        avatar_url: Option<&str>,
        preferred_locale: &str,
        password_hash: &str,
    ) -> sqlx::Result<User> {
        sqlx::query_as::<_, User>(
            r#"
            INSERT INTO users (
                organization_id, email, first_name, last_name, phone_number,
                avatar_url, preferred_locale, password_hash, status
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'active')
            RETURNING *
            "#,
        )
        .bind(organization_id)
        .bind(email)
        .bind(first_name)
        .bind(last_name)
        .bind(phone_number)
        .bind(avatar_url)
        .bind(preferred_locale)
        .bind(password_hash)
        .fetch_one(conn)
        .await
    }

    /// Complete a previously invited row.
    #[allow(clippy::too_many_arguments)]
    pub async fn activate(
        conn: &mut PgConnection,
        organization_id: Uuid,
        id: Uuid,
        first_name: &str,
        last_name: &str,
        phone_number: Option<&str>,
        avatar_url: Option<&str>,
        preferred_locale: &str,
        password_hash: &str,
    ) -> sqlx::Result<User> {
        sqlx::query_as::<_, User>(
            r#"
            UPDATE users SET
                first_name = $3, last_name = $4, phone_number = $5, avatar_url = $6,
                preferred_locale = $7, password_hash = $8, status = 'active',
                email_verified_at = COALESCE(email_verified_at, now()),
                updated_at = now()
            WHERE organization_id = $1 AND id = $2
            RETURNING *
            "#,
        )
        .bind(organization_id)
        .bind(id)
        .bind(first_name)
        .bind(last_name)
        .bind(phone_number)
        .bind(avatar_url)
        .bind(preferred_locale)
        .bind(password_hash)
        .fetch_one(conn)
        .await
    }

    pub async fn touch_login(pool: &PgPool, organization_id: Uuid, id: Uuid) -> sqlx::Result<()> {
        sqlx::query(
            "UPDATE users SET last_login_at = now(), updated_at = now() WHERE organization_id = $1 AND id = $2",
        )
        .bind(organization_id)
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Suspend or reactivate a user. Suspension always revokes their sessions.
    pub async fn set_status(
        pool: &PgPool,
        organization_id: Uuid,
        id: Uuid,
        status: &str,
    ) -> sqlx::Result<Option<User>> {
        let mut tx = pool.begin().await?;
        let user = sqlx::query_as::<_, User>(
            r#"
            UPDATE users SET status = $3, updated_at = now()
            WHERE organization_id = $1 AND id = $2
            RETURNING *
            "#,
        )
        .bind(organization_id)
        .bind(id)
        .bind(status)
        .fetch_optional(&mut *tx)
        .await?;

        if status != "active" {
            sqlx::query(
                "UPDATE sessions SET revoked_at = now() WHERE organization_id = $1 AND user_id = $2 AND revoked_at IS NULL",
            )
            .bind(organization_id)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(user)
    }
}

pub mod role {
    use super::*;

    /// System roles (organization_id IS NULL) plus the tenant's own roles.
    pub async fn list_for_org(pool: &PgPool, organization_id: Uuid) -> sqlx::Result<Vec<Role>> {
        sqlx::query_as::<_, Role>(
            "SELECT * FROM roles WHERE organization_id IS NULL OR organization_id = $1 ORDER BY code",
        )
        .bind(organization_id)
        .fetch_all(pool)
        .await
    }

    pub async fn find_by_code(pool: &PgPool, code: &str) -> sqlx::Result<Option<Role>> {
        sqlx::query_as::<_, Role>("SELECT * FROM roles WHERE code = $1")
            .bind(code)
            .fetch_optional(pool)
            .await
    }

    pub async fn find_for_org(
        pool: &PgPool,
        organization_id: Uuid,
        role_id: Uuid,
    ) -> sqlx::Result<Option<Role>> {
        sqlx::query_as::<_, Role>(
            "SELECT * FROM roles WHERE id = $1 AND (organization_id IS NULL OR organization_id = $2)",
        )
        .bind(role_id)
        .bind(organization_id)
        .fetch_optional(pool)
        .await
    }

    pub async fn permissions_for_user(
        pool: &PgPool,
        organization_id: Uuid,
        user_id: Uuid,
    ) -> sqlx::Result<Vec<String>> {
        sqlx::query_scalar::<_, String>(
            r#"
            SELECT DISTINCT p.code
            FROM user_roles ur
            JOIN role_permissions rp ON rp.role_id = ur.role_id
            JOIN permissions p ON p.id = rp.permission_id
            WHERE ur.organization_id = $1 AND ur.user_id = $2
            ORDER BY p.code
            "#,
        )
        .bind(organization_id)
        .bind(user_id)
        .fetch_all(pool)
        .await
    }

    pub async fn roles_for_user(
        pool: &PgPool,
        organization_id: Uuid,
        user_id: Uuid,
    ) -> sqlx::Result<Vec<Role>> {
        sqlx::query_as::<_, Role>(
            r#"
            SELECT r.* FROM roles r
            JOIN user_roles ur ON ur.role_id = r.id
            WHERE ur.organization_id = $1 AND ur.user_id = $2
            ORDER BY r.code
            "#,
        )
        .bind(organization_id)
        .bind(user_id)
        .fetch_all(pool)
        .await
    }

    pub async fn assign(
        conn: &mut PgConnection,
        organization_id: Uuid,
        user_id: Uuid,
        role_id: Uuid,
        assigned_by: Option<Uuid>,
    ) -> sqlx::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO user_roles (organization_id, user_id, role_id, assigned_by)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (organization_id, user_id, role_id) DO NOTHING
            "#,
        )
        .bind(organization_id)
        .bind(user_id)
        .bind(role_id)
        .bind(assigned_by)
        .execute(conn)
        .await?;
        Ok(())
    }
}

pub mod invitation {
    use super::*;

    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        pool: &PgPool,
        organization_id: Uuid,
        email: &str,
        phone_number: Option<&str>,
        role_id: Uuid,
        token_hash: &str,
        invited_by: Uuid,
        expires_at: DateTime<Utc>,
    ) -> sqlx::Result<Invitation> {
        sqlx::query_as::<_, Invitation>(
            r#"
            INSERT INTO invitations (
                organization_id, email, phone_number, role_id, token_hash, invited_by, expires_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING *
            "#,
        )
        .bind(organization_id)
        .bind(email)
        .bind(phone_number)
        .bind(role_id)
        .bind(token_hash)
        .bind(invited_by)
        .bind(expires_at)
        .fetch_one(pool)
        .await
    }

    pub async fn find_pending_by_email(
        pool: &PgPool,
        organization_id: Uuid,
        email: &str,
    ) -> sqlx::Result<Option<Invitation>> {
        sqlx::query_as::<_, Invitation>(
            "SELECT * FROM invitations WHERE organization_id = $1 AND email = $2 AND status = 'pending'",
        )
        .bind(organization_id)
        .bind(email)
        .fetch_optional(pool)
        .await
    }

    pub async fn list_for_org(
        pool: &PgPool,
        organization_id: Uuid,
    ) -> sqlx::Result<Vec<Invitation>> {
        sqlx::query_as::<_, Invitation>(
            "SELECT * FROM invitations WHERE organization_id = $1 ORDER BY created_at DESC",
        )
        .bind(organization_id)
        .fetch_all(pool)
        .await
    }

    /// Lock the invite row so a token can only be consumed once.
    pub async fn lock_by_token(
        conn: &mut PgConnection,
        token_hash: &str,
    ) -> sqlx::Result<Option<Invitation>> {
        sqlx::query_as::<_, Invitation>(
            "SELECT * FROM invitations WHERE token_hash = $1 FOR UPDATE",
        )
        .bind(token_hash)
        .fetch_optional(conn)
        .await
    }

    pub async fn find_by_token(
        pool: &PgPool,
        token_hash: &str,
    ) -> sqlx::Result<Option<Invitation>> {
        sqlx::query_as::<_, Invitation>("SELECT * FROM invitations WHERE token_hash = $1")
            .bind(token_hash)
            .fetch_optional(pool)
            .await
    }

    pub async fn mark_status(pool: &PgPool, id: Uuid, status: &str) -> sqlx::Result<()> {
        sqlx::query("UPDATE invitations SET status = $2 WHERE id = $1")
            .bind(id)
            .bind(status)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn accept(
        conn: &mut PgConnection,
        organization_id: Uuid,
        id: Uuid,
    ) -> sqlx::Result<()> {
        sqlx::query(
            "UPDATE invitations SET status = 'accepted', accepted_at = now() WHERE organization_id = $1 AND id = $2",
        )
        .bind(organization_id)
        .bind(id)
        .execute(conn)
        .await?;
        Ok(())
    }

    pub async fn revoke(
        pool: &PgPool,
        organization_id: Uuid,
        id: Uuid,
    ) -> sqlx::Result<Option<Invitation>> {
        sqlx::query_as::<_, Invitation>(
            "UPDATE invitations SET status = 'revoked' WHERE organization_id = $1 AND id = $2 AND status = 'pending' RETURNING *",
        )
        .bind(organization_id)
        .bind(id)
        .fetch_optional(pool)
        .await
    }
}

pub mod session {
    use super::*;

    pub struct NewSession<'a> {
        pub user_id: Uuid,
        pub organization_id: Uuid,
        pub token_hash: &'a str,
        pub ip_address: Option<&'a str>,
        pub user_agent: Option<&'a str>,
        pub expires_at: DateTime<Utc>,
    }

    pub async fn create(pool: &PgPool, new: NewSession<'_>) -> sqlx::Result<Session> {
        sqlx::query_as::<_, Session>(
            r#"
            INSERT INTO sessions (user_id, organization_id, token_hash, ip_address, user_agent, expires_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING *
            "#,
        )
        .bind(new.user_id)
        .bind(new.organization_id)
        .bind(new.token_hash)
        .bind(new.ip_address)
        .bind(new.user_agent)
        .bind(new.expires_at)
        .fetch_one(pool)
        .await
    }

    /// Resolve a live session by token hash. Returns (session, user).
    pub async fn find_live_by_token(
        pool: &PgPool,
        token_hash: &str,
    ) -> sqlx::Result<Option<(Session, User)>> {
        let session = sqlx::query_as::<_, Session>(
            r#"
            SELECT * FROM sessions
            WHERE token_hash = $1 AND revoked_at IS NULL AND expires_at > now()
            "#,
        )
        .bind(token_hash)
        .fetch_optional(pool)
        .await?;

        let Some(session) = session else {
            return Ok(None);
        };

        let user = sqlx::query_as::<_, User>(
            "SELECT * FROM users WHERE id = $1 AND organization_id = $2 AND status = 'active'",
        )
        .bind(session.user_id)
        .bind(session.organization_id)
        .fetch_optional(pool)
        .await?;

        Ok(user.map(|u| (session, u)))
    }

    pub async fn list_active(
        pool: &PgPool,
        organization_id: Uuid,
        user_id: Uuid,
    ) -> sqlx::Result<Vec<Session>> {
        sqlx::query_as::<_, Session>(
            r#"
            SELECT * FROM sessions
            WHERE organization_id = $1 AND user_id = $2
              AND revoked_at IS NULL AND expires_at > now()
            ORDER BY created_at DESC
            "#,
        )
        .bind(organization_id)
        .bind(user_id)
        .fetch_all(pool)
        .await
    }

    pub async fn revoke(
        pool: &PgPool,
        organization_id: Uuid,
        user_id: Uuid,
        id: Uuid,
    ) -> sqlx::Result<bool> {
        let result = sqlx::query(
            "UPDATE sessions SET revoked_at = now() WHERE organization_id = $1 AND user_id = $2 AND id = $3 AND revoked_at IS NULL",
        )
        .bind(organization_id)
        .bind(user_id)
        .bind(id)
        .execute(pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn revoke_all(
        pool: &PgPool,
        organization_id: Uuid,
        user_id: Uuid,
    ) -> sqlx::Result<u64> {
        let result = sqlx::query(
            "UPDATE sessions SET revoked_at = now() WHERE organization_id = $1 AND user_id = $2 AND revoked_at IS NULL",
        )
        .bind(organization_id)
        .bind(user_id)
        .execute(pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// Cross-tenant probe: was this session id issued to another tenant?
    pub async fn organization_of(pool: &PgPool, id: Uuid) -> sqlx::Result<Option<Uuid>> {
        sqlx::query_scalar::<_, Uuid>("SELECT organization_id FROM sessions WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await
    }
}

pub mod auth_token {
    use super::*;
    use crate::entity::AuthToken;

    /// Issue a single-use token, superseding the previous live one for the same
    /// user/purpose. Only the SHA-256 hash is stored.
    pub async fn replace_live(
        pool: &PgPool,
        organization_id: Uuid,
        user_id: Uuid,
        purpose: &str,
        token_hash: &str,
        expires_at: DateTime<Utc>,
    ) -> sqlx::Result<AuthToken> {
        let mut tx = pool.begin().await?;
        sqlx::query(
            "DELETE FROM auth_tokens
             WHERE organization_id = $1 AND user_id = $2 AND purpose = $3 AND consumed_at IS NULL",
        )
        .bind(organization_id)
        .bind(user_id)
        .bind(purpose)
        .execute(&mut *tx)
        .await?;

        let row = sqlx::query_as::<_, AuthToken>(
            r#"
            INSERT INTO auth_tokens (organization_id, user_id, token_hash, purpose, expires_at)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING *
            "#,
        )
        .bind(organization_id)
        .bind(user_id)
        .bind(token_hash)
        .bind(purpose)
        .bind(expires_at)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(row)
    }

    /// Lock the token row so a link can only be redeemed once.
    pub async fn lock_by_hash(
        conn: &mut PgConnection,
        token_hash: &str,
    ) -> sqlx::Result<Option<AuthToken>> {
        sqlx::query_as::<_, AuthToken>("SELECT * FROM auth_tokens WHERE token_hash = $1 FOR UPDATE")
            .bind(token_hash)
            .fetch_optional(conn)
            .await
    }

    pub async fn consume(
        conn: &mut PgConnection,
        organization_id: Uuid,
        id: Uuid,
    ) -> sqlx::Result<bool> {
        let result = sqlx::query(
            "UPDATE auth_tokens SET consumed_at = now()
             WHERE organization_id = $1 AND id = $2 AND consumed_at IS NULL",
        )
        .bind(organization_id)
        .bind(id)
        .execute(conn)
        .await?;
        Ok(result.rows_affected() > 0)
    }
}

pub mod audit {
    use super::*;

    pub struct Entry<'a> {
        pub organization_id: Option<Uuid>,
        pub actor_user_id: Option<Uuid>,
        pub action: &'a str,
        pub entity_type: &'a str,
        pub entity_id: Option<Uuid>,
        pub metadata: serde_json::Value,
        pub ip_address: Option<&'a str>,
    }

    pub async fn insert(pool: &PgPool, entry: Entry<'_>) -> sqlx::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO audit_logs (
                organization_id, actor_user_id, action, entity_type, entity_id, metadata,
                ip_address, actor_user_agent
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7,
                    NULLIF(current_setting('app.user_agent', true), ''))
            "#,
        )
        .bind(entry.organization_id)
        .bind(entry.actor_user_id)
        .bind(entry.action)
        .bind(entry.entity_type)
        .bind(entry.entity_id)
        .bind(entry.metadata)
        .bind(entry.ip_address)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Audit inside the same transaction as the state change.
    pub async fn insert_tx(conn: &mut PgConnection, entry: Entry<'_>) -> sqlx::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO audit_logs (
                organization_id, actor_user_id, action, entity_type, entity_id, metadata,
                ip_address, actor_user_agent
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7,
                    NULLIF(current_setting('app.user_agent', true), ''))
            "#,
        )
        .bind(entry.organization_id)
        .bind(entry.actor_user_id)
        .bind(entry.action)
        .bind(entry.entity_type)
        .bind(entry.entity_id)
        .bind(entry.metadata)
        .bind(entry.ip_address)
        .execute(conn)
        .await?;
        Ok(())
    }

    pub async fn count_by_action(
        pool: &PgPool,
        organization_id: Option<Uuid>,
        action: &str,
    ) -> sqlx::Result<i64> {
        sqlx::query_scalar::<_, i64>(
            r#"
            SELECT count(*) FROM audit_logs
            WHERE action = $1 AND ($2::uuid IS NULL OR organization_id = $2)
            "#,
        )
        .bind(action)
        .bind(organization_id)
        .fetch_one(pool)
        .await
    }

    /// Who is making the request. Triggers read this from transaction-local
    /// settings, so every row change in the transaction is attributed.
    #[derive(Clone, Copy, Default)]
    pub struct Actor<'a> {
        pub user_id: Option<Uuid>,
        pub ip_address: Option<&'a str>,
        pub user_agent: Option<&'a str>,
    }

    /// Must be called inside a transaction: `set_config(..., true)` is
    /// transaction-local and never leaks into the pooled connection.
    pub async fn bind_actor(conn: &mut PgConnection, actor: Actor<'_>) -> sqlx::Result<()> {
        sqlx::query(
            r#"
            SELECT set_config('app.actor_user_id', $1, true),
                   set_config('app.request_ip', $2, true),
                   set_config('app.user_agent', $3, true)
            "#,
        )
        .bind(actor.user_id.map(|id| id.to_string()).unwrap_or_default())
        .bind(actor.ip_address.unwrap_or_default())
        .bind(actor.user_agent.unwrap_or_default())
        .execute(conn)
        .await?;
        Ok(())
    }
}
