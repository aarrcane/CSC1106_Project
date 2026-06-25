-- One-time migration for forum image attachments on existing Supabase databases.

CREATE TABLE IF NOT EXISTS forum_attachments (
    id SERIAL PRIMARY KEY,
    thread_id INT REFERENCES forum_threads(id) ON DELETE CASCADE,
    post_id INT REFERENCES forum_posts(id) ON DELETE CASCADE,
    uploaded_by INT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    object_path VARCHAR(500) UNIQUE NOT NULL,
    original_filename VARCHAR(255) NOT NULL,
    content_type VARCHAR(50) NOT NULL CHECK (content_type IN ('image/jpeg', 'image/png')),
    file_size INT NOT NULL CHECK (file_size > 0 AND file_size <= 5242880),
    deleted_at TIMESTAMPTZ,
    deleted_by INT REFERENCES users(id) ON DELETE SET NULL,
    delete_reason TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (
        (thread_id IS NOT NULL AND post_id IS NULL)
        OR (thread_id IS NULL AND post_id IS NOT NULL)
    )
);

ALTER TABLE forum_moderation_actions
    DROP CONSTRAINT IF EXISTS forum_moderation_actions_target_type_check;

ALTER TABLE forum_moderation_actions
    ADD CONSTRAINT forum_moderation_actions_target_type_check
    CHECK (target_type IN ('thread', 'post', 'attachment'));
