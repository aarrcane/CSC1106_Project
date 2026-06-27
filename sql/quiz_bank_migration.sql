-- Adaptive question-bank: serve a subset of a larger pool, with retakes.
-- Safe to run on an existing database (idempotent).

ALTER TABLE quizzes        ADD COLUMN IF NOT EXISTS serve_count      INT;                       -- NULL = serve all
ALTER TABLE quizzes        ADD COLUMN IF NOT EXISTS attempts_allowed INT NOT NULL DEFAULT 1;
ALTER TABLE quiz_questions ADD COLUMN IF NOT EXISTS difficulty       SMALLINT NOT NULL DEFAULT 1;
ALTER TABLE quiz_questions ADD COLUMN IF NOT EXISTS topic            VARCHAR(120);
ALTER TABLE quiz_attempts  ADD COLUMN IF NOT EXISTS total_marks      INT;

CREATE TABLE IF NOT EXISTS quiz_attempt_questions (
    attempt_id  INT NOT NULL REFERENCES quiz_attempts(id) ON DELETE CASCADE,
    question_id INT NOT NULL REFERENCES quiz_questions(id) ON DELETE CASCADE,
    position    INT NOT NULL,
    PRIMARY KEY (attempt_id, question_id)
);
