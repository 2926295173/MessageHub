-- Add a stable per-physical-device identifier so the daemon can
-- dedupe `device.hello` reconnects that come with a regenerated
-- `device_id` UUID (the UUID is wiped on `pm clear` because the
-- Keystore is wiped, but the Android `ANDROID_ID` it survives
-- `pm clear` since it is keyed by the app's signing key).
--
-- The column is NULL for rows created by older clients that did
-- not send the new field, so the migration preserves historical
-- data. The `upsert_device` SQL falls back to matching on
-- `device_id` when `hardware_id` is NULL.

ALTER TABLE devices ADD COLUMN hardware_id TEXT;

-- Index on the new column. Partial (NULL rows skipped) so the
-- index stays small for legacy data.
CREATE INDEX IF NOT EXISTS devices_hardware_id_idx
    ON devices(hardware_id) WHERE hardware_id IS NOT NULL;
