-- One-time migration for existing Supabase databases before using the forum feature.

ALTER TABLE notifications
    ADD COLUMN IF NOT EXISTS link_url VARCHAR(500);

ALTER TABLE forum_threads
    ADD COLUMN IF NOT EXISTS thread_type VARCHAR(20) NOT NULL DEFAULT 'discussion',
    ADD COLUMN IF NOT EXISTS locked_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS locked_by INT REFERENCES users(id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS edited_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS deleted_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS deleted_by INT REFERENCES users(id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS delete_reason TEXT;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'forum_threads_thread_type_check'
    ) THEN
        ALTER TABLE forum_threads
            ADD CONSTRAINT forum_threads_thread_type_check
            CHECK (thread_type IN ('discussion', 'announcement'));
    END IF;
END $$;

ALTER TABLE forum_posts
    ADD COLUMN IF NOT EXISTS parent_post_id INT REFERENCES forum_posts(id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS edited_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS deleted_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS deleted_by INT REFERENCES users(id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS delete_reason TEXT;

CREATE TABLE IF NOT EXISTS forum_moderation_actions (
    id SERIAL PRIMARY KEY,
    moderator_user_id INT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    action VARCHAR(40) NOT NULL CHECK (action IN ('delete', 'pin', 'unpin', 'answered', 'unanswered', 'lock', 'unlock')),
    target_type VARCHAR(20) NOT NULL CHECK (target_type IN ('thread', 'post')),
    target_id INT NOT NULL,
    thread_id INT REFERENCES forum_threads(id) ON DELETE CASCADE,
    reason TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
