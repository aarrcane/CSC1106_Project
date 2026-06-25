-- Rollback for sql/forum_attachments_migration.sql.
-- This removes attachment metadata only. Supabase Storage objects should be
-- deleted separately if attachment uploads were already used.

DROP TABLE IF EXISTS forum_attachments;

ALTER TABLE forum_moderation_actions
    DROP CONSTRAINT IF EXISTS forum_moderation_actions_target_type_check;

ALTER TABLE forum_moderation_actions
    ADD CONSTRAINT forum_moderation_actions_target_type_check
    CHECK (target_type IN ('thread', 'post'));
