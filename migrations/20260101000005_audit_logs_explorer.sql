-- Milestone 4: turn `audit_logs` into an explorable change feed.
-- The table keeps its original columns; the explorer only adds what it needs.

ALTER TABLE audit_logs
    ADD COLUMN IF NOT EXISTS actor_user_agent TEXT,
    ADD COLUMN IF NOT EXISTS old_values JSONB,
    ADD COLUMN IF NOT EXISTS new_values JSONB,
    ADD COLUMN IF NOT EXISTS diff JSONB;

-- Filter columns from the Audit Log Explorer.
CREATE INDEX IF NOT EXISTS audit_logs_org_actor_created_idx
    ON audit_logs (organization_id, actor_user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS audit_logs_org_entity_created_idx
    ON audit_logs (organization_id, entity_type, entity_id, created_at DESC);
CREATE INDEX IF NOT EXISTS audit_logs_org_event_created_idx
    ON audit_logs (organization_id, action, created_at DESC);

-- Free-text search over old/new/diff values.
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE INDEX IF NOT EXISTS audit_logs_diff_trgm_idx
    ON audit_logs USING GIN ((diff::text) gin_trgm_ops);
CREATE INDEX IF NOT EXISTS audit_logs_old_values_trgm_idx
    ON audit_logs USING GIN ((old_values::text) gin_trgm_ops);
CREATE INDEX IF NOT EXISTS audit_logs_new_values_trgm_idx
    ON audit_logs USING GIN ((new_values::text) gin_trgm_ops);

-- --------------------------------------------------------------- trigger -----
-- One generic AFTER trigger per audited table. It records the changed columns
-- only (never the whole row twice) and takes the request actor from
-- transaction-local GUCs set by `audit::bind_actor`.
CREATE OR REPLACE FUNCTION audit_row_change() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
DECLARE
    old_row   JSONB;
    new_row   JSONB;
    changed   JSONB := '{}'::jsonb;
    has_delta BOOLEAN := false;
    entity    TEXT;
    actor     UUID;
    org       UUID;
BEGIN
    IF TG_OP = 'INSERT' THEN
        new_row := to_jsonb(NEW);
        SELECT COALESCE(jsonb_object_agg(key, jsonb_build_object('new', value)), '{}'::jsonb)
          INTO changed FROM jsonb_each(new_row);
        has_delta := true;
    ELSIF TG_OP = 'UPDATE' THEN
        old_row := to_jsonb(OLD);
        new_row := to_jsonb(NEW);
        SELECT COALESCE(jsonb_object_agg(n.key,
                   jsonb_build_object('old', o.value, 'new', n.value)), '{}'::jsonb),
               count(*) > 0
          INTO changed, has_delta
          FROM jsonb_each(new_row) n
          JOIN jsonb_each(old_row) o ON o.key = n.key
         WHERE n.value IS DISTINCT FROM o.value
           AND n.key NOT IN ('updated_at', 'created_at', 'last_login_at');
    ELSE
        old_row := to_jsonb(OLD);
        SELECT COALESCE(jsonb_object_agg(key, jsonb_build_object('old', value)), '{}'::jsonb)
          INTO changed FROM jsonb_each(old_row);
        has_delta := true;
    END IF;

    -- An UPDATE that only touched bookkeeping columns is not an event.
    IF NOT has_delta THEN
        RETURN NULL;
    END IF;

    entity := CASE TG_TABLE_NAME
        WHEN 'assets' THEN 'asset'
        WHEN 'products' THEN 'product'
        WHEN 'departments' THEN 'department'
        WHEN 'categories' THEN 'category'
        WHEN 'manufacturers' THEN 'manufacturer'
        WHEN 'statuses' THEN 'status'
        WHEN 'locations' THEN 'location'
        WHEN 'suppliers' THEN 'supplier'
        WHEN 'depreciation_models' THEN 'depreciation_model'
        WHEN 'accessories' THEN 'accessory'
        WHEN 'consumables' THEN 'consumable'
        WHEN 'licenses' THEN 'license'
        WHEN 'users' THEN 'user'
        WHEN 'asset_maintenances' THEN 'maintenance'
        WHEN 'attachments' THEN 'attachment'
        ELSE TG_TABLE_NAME
    END;

    org := (COALESCE(new_row, old_row) ->> 'organization_id')::uuid;
    actor := NULLIF(current_setting('app.actor_user_id', true), '')::uuid;

    INSERT INTO audit_logs (
        organization_id, actor_user_id, actor_user_agent, action, entity_type,
        entity_id, metadata, old_values, new_values, diff, ip_address
    ) VALUES (
        org,
        actor,
        NULLIF(current_setting('app.user_agent', true), ''),
        CASE TG_OP WHEN 'INSERT' THEN 'created' WHEN 'UPDATE' THEN 'updated' ELSE 'deleted' END,
        entity,
        (COALESCE(new_row, old_row) ->> 'id')::uuid,
        jsonb_build_object('source', 'trigger'),
        CASE WHEN TG_OP = 'INSERT' THEN NULL ELSE old_row END,
        CASE WHEN TG_OP = 'DELETE' THEN NULL ELSE new_row END,
        changed,
        NULLIF(current_setting('app.request_ip', true), '')
    );
    RETURN NULL;
END;
$$;

DO $$
DECLARE t TEXT;
BEGIN
    FOREACH t IN ARRAY ARRAY[
        'assets', 'products', 'departments', 'categories', 'manufacturers',
        'statuses', 'locations', 'suppliers', 'depreciation_models',
        'accessories', 'consumables', 'licenses', 'users',
        'asset_maintenances', 'attachments'
    ] LOOP
        IF to_regclass(t) IS NOT NULL THEN
            EXECUTE format(
                'DROP TRIGGER IF EXISTS %I ON %I', t || '_audit', t
            );
            EXECUTE format(
                'CREATE TRIGGER %I AFTER INSERT OR UPDATE OR DELETE ON %I
                 FOR EACH ROW EXECUTE FUNCTION audit_row_change()',
                t || '_audit', t
            );
        END IF;
    END LOOP;
END;
$$;

-- ------------------------------------------------------------ normalise -----
-- Existing rows used `entity.verb`; the explorer reads short event names.
UPDATE audit_logs
   SET action = CASE split_part(action, '.', 2)
        WHEN 'create' THEN 'created'
        WHEN 'update' THEN 'updated'
        WHEN 'delete' THEN 'deleted'
        WHEN 'assign' THEN 'assigned'
        WHEN 'return' THEN 'returned'
        WHEN 'transfer' THEN 'transferred'
        WHEN 'revoke' THEN 'revoked'
        WHEN 'login' THEN 'login'
        WHEN 'login_failed' THEN 'login_failed'
        WHEN 'logout' THEN 'logout'
        WHEN 'seeded' THEN 'seeded'
        WHEN 'bootstrapped' THEN 'bootstrapped'
        WHEN 'accepted' THEN 'accepted'
        WHEN 'created' THEN 'created'
        WHEN 'revoked' THEN 'revoked'
        WHEN 'status_changed' THEN 'status_changed'
        WHEN 'cross_access_denied' THEN 'cross_access_denied'
        ELSE split_part(action, '.', 2)
   END
 WHERE action LIKE '%.%';
