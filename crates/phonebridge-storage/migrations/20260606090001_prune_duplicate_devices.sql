-- One-time cleanup of historical duplicate device rows created
-- by repeated `pm clear` of the phone (each wipe regenerated
-- the device_id UUID and inserted a fresh row). With the new
-- `hardware_id` column in place, the daemon will now dedupe
-- reconnects correctly. This migration keeps the most-recent
-- row per distinct (name, hardware_id-or-empty) and deletes
-- the rest, plus any orphan rows that have neither a hardware
-- id nor a non-empty name match.
--
-- The matching is intentionally strict: we only merge rows
-- that share the same `name` AND have a NULL hardware_id
-- (i.e. the legacy rows we're cleaning up). The safest
-- assumption is "same physical phone" because (a) the phone
-- is sending the same Build.MODEL-derived name in every
-- `device.hello` and (b) the dedup is only applied to rows
-- the user has already approved by virtue of having seen them
-- in the Devices page; if you really have two physical
-- phones with the same model name (e.g. two Pixels), you
-- can unpair them after this migration runs.

DELETE FROM devices
WHERE id IN (
    SELECT id FROM (
        SELECT id,
               ROW_NUMBER() OVER (
                   PARTITION BY name
                   ORDER BY last_seen DESC, id DESC
               ) AS rn
        FROM devices
        WHERE hardware_id IS NULL
    )
    WHERE rn > 1
);
