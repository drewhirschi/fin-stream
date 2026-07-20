-- Failed scheduled slots become retryable. Error rows leave the slot
-- uniqueness index, so a later cron redelivery can claim the same slot again;
-- the one-running index still guarantees a single concurrent run, and the
-- claim SQL caps the number of recorded error attempts per slot.
DROP INDEX IF EXISTS sync_log_one_scheduled_slot_idx;
CREATE UNIQUE INDEX sync_log_one_scheduled_slot_idx
    ON sync_log (connection_slug, scheduled_for)
    WHERE scheduled_for IS NOT NULL AND status IN ('running', 'success');
