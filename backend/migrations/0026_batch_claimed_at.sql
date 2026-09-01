-- Batch-claim watchdog: ai_tag_batches gains `claimed_at` so the worker can
-- reclaim batches orphaned by a crash/restart (status stuck in 'processing'
-- from a dead instance) or by a hung AI call.

ALTER TABLE ai_tag_batches ADD COLUMN claimed_at timestamptz;
