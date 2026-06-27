-- ============================================================================
-- practice_migration.sql
-- Adds "practice quiz" support: a per-quiz is_practice flag, a separate
-- practice proficiency track, and per-attempt before/after snapshots.
-- Idempotent: safe to run on an existing database.
-- ============================================================================

-- 1. Mark a quiz as a practice quiz (adaptive difficulty, unlimited attempts,
--    no proctoring / no open-close window enforcement).
ALTER TABLE quizzes ADD COLUMN IF NOT EXISTS is_practice BOOLEAN NOT NULL DEFAULT FALSE;

-- 2. Practice-only proficiency, kept separate from graded student_topic_proficiency.
CREATE TABLE IF NOT EXISTS student_practice_proficiency (
    id             SERIAL PRIMARY KEY,
    student_id     INT NOT NULL REFERENCES students(id) ON DELETE CASCADE,
    course_id      INT NOT NULL REFERENCES courses(id)  ON DELETE CASCADE,
    topic          VARCHAR(120) NOT NULL,
    proficiency    NUMERIC(4,3) NOT NULL DEFAULT 0.5,  -- 0.000..1.000
    answered_count INT NOT NULL DEFAULT 0,
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (student_id, course_id, topic)   -- required for the ON CONFLICT upsert
);

-- 3. Before/after proficiency per attempt, for the "your proficiency changed"
--    readout on the practice result page.
CREATE TABLE IF NOT EXISTS practice_attempt_proficiency (
    attempt_id  INT NOT NULL REFERENCES quiz_attempts(id) ON DELETE CASCADE,
    topic       VARCHAR(120) NOT NULL,
    prof_before NUMERIC(4,3) NOT NULL,
    prof_after  NUMERIC(4,3) NOT NULL,
    PRIMARY KEY (attempt_id, topic)
);
