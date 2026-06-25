-- Rollback for sql/forum_discussion_migration.sql.
-- This removes the forum feature additions. Running it after real forum usage
-- will delete moderation logs and the added metadata columns.

DROP TABLE IF EXISTS forum_moderation_actions;

ALTER TABLE forum_posts
    DROP COLUMN IF EXISTS delete_reason,
    DROP COLUMN IF EXISTS deleted_by,
    DROP COLUMN IF EXISTS deleted_at,
    DROP COLUMN IF EXISTS edited_at,
    DROP COLUMN IF EXISTS parent_post_id;

ALTER TABLE forum_threads
    DROP CONSTRAINT IF EXISTS forum_threads_thread_type_check,
    DROP COLUMN IF EXISTS delete_reason,
    DROP COLUMN IF EXISTS deleted_by,
    DROP COLUMN IF EXISTS deleted_at,
    DROP COLUMN IF EXISTS edited_at,
    DROP COLUMN IF EXISTS locked_by,
    DROP COLUMN IF EXISTS locked_at,
    DROP COLUMN IF EXISTS thread_type;

ALTER TABLE notifications
    DROP COLUMN IF EXISTS link_url;
