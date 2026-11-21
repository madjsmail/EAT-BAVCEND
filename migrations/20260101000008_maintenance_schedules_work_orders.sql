-- Schedule-driven, event-sourced maintenance workflow.
--
-- The existing `asset_maintenances` ticket table stays in place; this migration
-- is additive. `maintenance_schedules` drives preventive work orders, while
-- `work_orders` / `work_order_events` are the board and append-only timeline.

ALTER TABLE assets
    ADD COLUMN IF NOT EXISTS current_operating_hours NUMERIC NOT NULL DEFAULT 0
    CHECK (current_operating_hours >= 0);

-- --------------------------------------------------- maintenance schedules ----
CREATE TABLE maintenance_schedules (
    id                        UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    asset_id                  UUID NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    frequency_type            TEXT NOT NULL
                              CHECK (frequency_type IN ('days', 'months', 'operating_hours')),
    interval                  INTEGER NOT NULL CHECK (interval > 0),
    next_due_at               TIMESTAMPTZ,
    next_due_operating_hours  NUMERIC,
    is_active                 BOOLEAN NOT NULL DEFAULT TRUE,
    notes                     TEXT,
    created_at                TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at                TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT maintenance_schedules_due_check CHECK (
        (frequency_type IN ('days', 'months') AND next_due_at IS NOT NULL)
        OR (frequency_type = 'operating_hours' AND next_due_operating_hours IS NOT NULL)
    )
);

CREATE INDEX maintenance_schedules_cron_idx
    ON maintenance_schedules (is_active, next_due_at);
CREATE INDEX maintenance_schedules_asset_idx
    ON maintenance_schedules (asset_id, created_at DESC);

-- ------------------------------------------------------------- work orders ----
CREATE TABLE work_orders (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    schedule_id   UUID REFERENCES maintenance_schedules(id) ON DELETE SET NULL,
    asset_id      UUID NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    type          TEXT NOT NULL CHECK (type IN ('preventive', 'corrective')),
    status        TEXT NOT NULL DEFAULT 'open'
                  CHECK (status IN ('open', 'in_progress', 'done')),
    assigned_to   UUID REFERENCES users(id) ON DELETE SET NULL,
    due_date      TIMESTAMPTZ,
    completed_at  TIMESTAMPTZ,
    cost          NUMERIC(12, 2) CHECK (cost IS NULL OR cost >= 0),
    notes         TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX work_orders_asset_created_idx
    ON work_orders (asset_id, created_at DESC);
CREATE INDEX work_orders_status_due_idx
    ON work_orders (status, due_date);
CREATE INDEX work_orders_assignee_status_idx
    ON work_orders (assigned_to, status);
-- One live preventive work order per schedule/due window.
CREATE UNIQUE INDEX work_orders_preventive_due_key
    ON work_orders (schedule_id, due_date)
    WHERE schedule_id IS NOT NULL AND status <> 'done';

-- ------------------------------------------------------- work order events ----
CREATE TABLE work_order_events (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    work_order_id  UUID NOT NULL REFERENCES work_orders(id) ON DELETE CASCADE,
    event_type     TEXT NOT NULL
                   CHECK (event_type IN ('created', 'assigned', 'status_changed',
                                         'cost_recorded', 'note_added', 'completed')),
    actor_id       UUID REFERENCES users(id) ON DELETE SET NULL,
    payload        JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX work_order_events_work_order_idx
    ON work_order_events (work_order_id, created_at ASC);

-- --------------------------------------------------------- schedule events ----
CREATE TABLE schedule_events (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    schedule_id  UUID NOT NULL REFERENCES maintenance_schedules(id) ON DELETE CASCADE,
    event_type   TEXT NOT NULL
                 CHECK (event_type IN ('schedule_created', 'schedule_updated',
                                       'schedule_advanced', 'schedule_deactivated')),
    actor_id     UUID REFERENCES users(id) ON DELETE SET NULL,
    payload      JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX schedule_events_schedule_idx
    ON schedule_events (schedule_id, created_at ASC);

-- ------------------------------------------------------- append-only guard ----
CREATE OR REPLACE FUNCTION prevent_append_only_change()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION '% is append-only and cannot be modified', TG_TABLE_NAME;
END;
$$;

DROP TRIGGER IF EXISTS work_order_events_append_only ON work_order_events;
CREATE TRIGGER work_order_events_append_only
    BEFORE UPDATE OR DELETE ON work_order_events
    FOR EACH ROW EXECUTE FUNCTION prevent_append_only_change();

DROP TRIGGER IF EXISTS schedule_events_append_only ON schedule_events;
CREATE TRIGGER schedule_events_append_only
    BEFORE UPDATE OR DELETE ON schedule_events
    FOR EACH ROW EXECUTE FUNCTION prevent_append_only_change();
