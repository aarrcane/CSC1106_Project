-- Align an existing development/Supabase database with the normalized LMS schema.
-- For the cleanest database, reset the dev database and run schema.sql instead.
-- This migration is intentionally additive where possible so existing prototype data is not dropped.

ALTER TABLE students ADD COLUMN IF NOT EXISTS student_no VARCHAR(50);
ALTER TABLE students ADD COLUMN IF NOT EXISTS programme VARCHAR(100);
ALTER TABLE students ADD COLUMN IF NOT EXISTS year_of_study INT;
ALTER TABLE students ADD COLUMN IF NOT EXISTS date_of_birth DATE;
ALTER TABLE students ADD COLUMN IF NOT EXISTS created_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
ALTER TABLE students ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW();

UPDATE students
SET student_no = COALESCE(student_no, 'S' || LPAD(id::TEXT, 6, '0'))
WHERE student_no IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_students_student_no_unique ON students (student_no);
CREATE INDEX IF NOT EXISTS idx_students_user_id ON students (user_id);

ALTER TABLE lecturers ADD COLUMN IF NOT EXISTS created_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
ALTER TABLE lecturers ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
CREATE INDEX IF NOT EXISTS idx_lecturers_user_id ON lecturers (user_id);

CREATE TABLE IF NOT EXISTS admins (
    id SERIAL PRIMARY KEY,
    user_id INT NOT NULL UNIQUE REFERENCES users(id) ON DELETE CASCADE,
    staff_no VARCHAR(50) UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS academic_terms (
    id SERIAL PRIMARY KEY,
    term_code VARCHAR(30) NOT NULL UNIQUE,
    name VARCHAR(100) NOT NULL,
    starts_on DATE NOT NULL,
    ends_on DATE NOT NULL,
    CHECK (ends_on > starts_on)
);

ALTER TABLE courses ADD COLUMN IF NOT EXISTS credits INT NOT NULL DEFAULT 15 CHECK (credits > 0);
ALTER TABLE courses ADD COLUMN IF NOT EXISTS created_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
ALTER TABLE courses ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW();

CREATE TABLE IF NOT EXISTS course_offerings (
    id SERIAL PRIMARY KEY,
    course_id INT NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
    term_id INT NOT NULL REFERENCES academic_terms(id) ON DELETE RESTRICT,
    lecturer_id INT NOT NULL REFERENCES lecturers(id) ON DELETE RESTRICT,
    section VARCHAR(20) NOT NULL DEFAULT 'A',
    capacity INT CHECK (capacity IS NULL OR capacity > 0),
    status VARCHAR(20) NOT NULL DEFAULT 'preparing'
        CHECK (status IN ('preparing', 'open', 'ongoing', 'completed', 'archived')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (course_id, term_id, section)
);

CREATE INDEX IF NOT EXISTS idx_course_offerings_course_id ON course_offerings (course_id);
CREATE INDEX IF NOT EXISTS idx_course_offerings_lecturer_id ON course_offerings (lecturer_id);

ALTER TABLE enrollments ADD COLUMN IF NOT EXISTS course_offering_id INT REFERENCES course_offerings(id) ON DELETE CASCADE;
CREATE INDEX IF NOT EXISTS idx_enrollments_student_id ON enrollments (student_id);
CREATE INDEX IF NOT EXISTS idx_enrollments_course_offering_id ON enrollments (course_offering_id);

CREATE TABLE IF NOT EXISTS learning_topics (
    id SERIAL PRIMARY KEY,
    course_id INT NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
    topic_code VARCHAR(50) NOT NULL,
    title VARCHAR(150) NOT NULL,
    description TEXT,
    display_order INT NOT NULL DEFAULT 0,
    UNIQUE (course_id, topic_code)
);

ALTER TABLE course_materials ADD COLUMN IF NOT EXISTS topic_id INT REFERENCES learning_topics(id) ON DELETE SET NULL;
ALTER TABLE course_materials ADD COLUMN IF NOT EXISTS storage_path VARCHAR(500);
ALTER TABLE course_materials ADD COLUMN IF NOT EXISTS original_filename VARCHAR(255);
ALTER TABLE course_materials ADD COLUMN IF NOT EXISTS mime_type VARCHAR(100);
ALTER TABLE course_materials ADD COLUMN IF NOT EXISTS file_size_bytes BIGINT CHECK (file_size_bytes IS NULL OR file_size_bytes >= 0);
UPDATE course_materials SET storage_path = file_path WHERE storage_path IS NULL AND file_path IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS idx_course_materials_course_storage_unique
    ON course_materials (course_id, storage_path)
    WHERE storage_path IS NOT NULL;

ALTER TABLE assignments ADD COLUMN IF NOT EXISTS course_offering_id INT REFERENCES course_offerings(id) ON DELETE CASCADE;
ALTER TABLE assignments ADD COLUMN IF NOT EXISTS due_at TIMESTAMPTZ;
ALTER TABLE assignments ADD COLUMN IF NOT EXISTS allow_late BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE assignments ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW();
UPDATE assignments SET due_at = due_date WHERE due_at IS NULL AND due_date IS NOT NULL;

ALTER TABLE submissions ADD COLUMN IF NOT EXISTS attempt_no INT NOT NULL DEFAULT 1 CHECK (attempt_no > 0);
ALTER TABLE submissions ADD COLUMN IF NOT EXISTS graded_by INT REFERENCES users(id) ON DELETE SET NULL;
ALTER TABLE submissions ADD COLUMN IF NOT EXISTS graded_at TIMESTAMPTZ;

CREATE TABLE IF NOT EXISTS submission_files (
    id SERIAL PRIMARY KEY,
    submission_id INT NOT NULL REFERENCES submissions(id) ON DELETE CASCADE,
    storage_path VARCHAR(500) NOT NULL,
    original_filename VARCHAR(255) NOT NULL,
    mime_type VARCHAR(100),
    file_size_bytes BIGINT CHECK (file_size_bytes IS NULL OR file_size_bytes >= 0),
    checksum_sha256 CHAR(64),
    uploaded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (submission_id, storage_path)
);

INSERT INTO submission_files (submission_id, storage_path, original_filename)
SELECT id, file_path, SPLIT_PART(file_path, '/', GREATEST(1, ARRAY_LENGTH(STRING_TO_ARRAY(file_path, '/'), 1)))
FROM submissions
WHERE file_path IS NOT NULL
ON CONFLICT DO NOTHING;

ALTER TABLE quizzes ADD COLUMN IF NOT EXISTS course_offering_id INT REFERENCES course_offerings(id) ON DELETE CASCADE;
ALTER TABLE quizzes ADD COLUMN IF NOT EXISTS duration_mins INT NOT NULL DEFAULT 25 CHECK (duration_mins > 0);
ALTER TABLE quizzes ADD COLUMN IF NOT EXISTS max_attempts INT NOT NULL DEFAULT 1 CHECK (max_attempts > 0);
ALTER TABLE quizzes ADD COLUMN IF NOT EXISTS adaptive_enabled BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE quizzes ADD COLUMN IF NOT EXISTS recommendation_enabled BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE quizzes ADD COLUMN IF NOT EXISTS status VARCHAR(20) NOT NULL DEFAULT 'draft'
    CHECK (status IN ('draft', 'published', 'closed', 'archived'));
ALTER TABLE quizzes ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW();

ALTER TABLE quiz_questions ADD COLUMN IF NOT EXISTS topic_id INT REFERENCES learning_topics(id) ON DELETE SET NULL;
ALTER TABLE quiz_questions ADD COLUMN IF NOT EXISTS position INT;
ALTER TABLE quiz_questions ADD COLUMN IF NOT EXISTS difficulty_level INT NOT NULL DEFAULT 1 CHECK (difficulty_level BETWEEN 1 AND 5);
ALTER TABLE quiz_questions ADD COLUMN IF NOT EXISTS explanation TEXT;
UPDATE quiz_questions SET position = id WHERE position IS NULL;

ALTER TABLE quiz_options ADD COLUMN IF NOT EXISTS position INT;
UPDATE quiz_options SET position = id WHERE position IS NULL;

ALTER TABLE quiz_attempts ADD COLUMN IF NOT EXISTS attempt_no INT NOT NULL DEFAULT 1 CHECK (attempt_no > 0);
ALTER TABLE quiz_attempts ADD COLUMN IF NOT EXISTS status VARCHAR(20) NOT NULL DEFAULT 'in_progress'
    CHECK (status IN ('in_progress', 'submitted', 'expired', 'graded'));
ALTER TABLE quiz_attempts ADD COLUMN IF NOT EXISTS difficulty_snapshot INT CHECK (difficulty_snapshot IS NULL OR difficulty_snapshot BETWEEN 1 AND 5);

ALTER TABLE quiz_answers ADD COLUMN IF NOT EXISTS marks_awarded NUMERIC(6,2) CHECK (marks_awarded IS NULL OR marks_awarded >= 0);
ALTER TABLE quiz_answers ADD COLUMN IF NOT EXISTS answered_at TIMESTAMPTZ NOT NULL DEFAULT NOW();

CREATE TABLE IF NOT EXISTS student_topic_performance (
    id SERIAL PRIMARY KEY,
    student_id INT NOT NULL REFERENCES students(id) ON DELETE CASCADE,
    topic_id INT NOT NULL REFERENCES learning_topics(id) ON DELETE CASCADE,
    attempts_count INT NOT NULL DEFAULT 0 CHECK (attempts_count >= 0),
    average_score_pct NUMERIC(5,2) CHECK (average_score_pct IS NULL OR average_score_pct BETWEEN 0 AND 100),
    mastery_level INT NOT NULL DEFAULT 1 CHECK (mastery_level BETWEEN 1 AND 5),
    last_attempt_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (student_id, topic_id)
);

CREATE TABLE IF NOT EXISTS recommendation_rules (
    id SERIAL PRIMARY KEY,
    course_id INT NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
    topic_id INT REFERENCES learning_topics(id) ON DELETE CASCADE,
    material_id INT REFERENCES course_materials(id) ON DELETE SET NULL,
    rule_name VARCHAR(150) NOT NULL,
    min_mastery_level INT NOT NULL DEFAULT 1 CHECK (min_mastery_level BETWEEN 1 AND 5),
    max_mastery_level INT NOT NULL DEFAULT 5 CHECK (max_mastery_level BETWEEN 1 AND 5),
    recommendation_type VARCHAR(30) NOT NULL
        CHECK (recommendation_type IN ('material', 'revision', 'practice_quiz', 'message')),
    message TEXT NOT NULL,
    priority INT NOT NULL DEFAULT 0,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    CHECK (min_mastery_level <= max_mastery_level)
);

CREATE TABLE IF NOT EXISTS student_recommendations (
    id SERIAL PRIMARY KEY,
    student_id INT NOT NULL REFERENCES students(id) ON DELETE CASCADE,
    rule_id INT REFERENCES recommendation_rules(id) ON DELETE SET NULL,
    topic_id INT REFERENCES learning_topics(id) ON DELETE SET NULL,
    material_id INT REFERENCES course_materials(id) ON DELETE SET NULL,
    reason TEXT NOT NULL,
    status VARCHAR(20) NOT NULL DEFAULT 'active'
        CHECK (status IN ('active', 'viewed', 'dismissed', 'completed')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS attendance_sessions (
    id SERIAL PRIMARY KEY,
    course_offering_id INT NOT NULL REFERENCES course_offerings(id) ON DELETE CASCADE,
    title VARCHAR(200) NOT NULL,
    starts_at TIMESTAMPTZ NOT NULL,
    ends_at TIMESTAMPTZ NOT NULL,
    created_by INT REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (course_offering_id, starts_at),
    CHECK (ends_at > starts_at)
);

CREATE TABLE IF NOT EXISTS attendance_records (
    id SERIAL PRIMARY KEY,
    attendance_session_id INT NOT NULL REFERENCES attendance_sessions(id) ON DELETE CASCADE,
    student_id INT NOT NULL REFERENCES students(id) ON DELETE CASCADE,
    status VARCHAR(20) NOT NULL CHECK (status IN ('present', 'late', 'absent', 'excused')),
    marked_by INT REFERENCES users(id) ON DELETE SET NULL,
    marked_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    remarks TEXT,
    UNIQUE (attendance_session_id, student_id)
);

ALTER TABLE forum_threads ADD COLUMN IF NOT EXISTS course_offering_id INT REFERENCES course_offerings(id) ON DELETE CASCADE;

CREATE TABLE IF NOT EXISTS forum_tags (
    id SERIAL PRIMARY KEY,
    name VARCHAR(50) NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS forum_thread_tags (
    thread_id INT NOT NULL REFERENCES forum_threads(id) ON DELETE CASCADE,
    tag_id INT NOT NULL REFERENCES forum_tags(id) ON DELETE CASCADE,
    PRIMARY KEY (thread_id, tag_id)
);

CREATE TABLE IF NOT EXISTS audit_logs (
    id SERIAL PRIMARY KEY,
    actor_user_id INT REFERENCES users(id) ON DELETE SET NULL,
    action VARCHAR(100) NOT NULL,
    entity_type VARCHAR(100) NOT NULL,
    entity_id INT,
    details JSONB,
    ip_address INET,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

ALTER TABLE final_grades ADD COLUMN IF NOT EXISTS course_offering_id INT REFERENCES course_offerings(id) ON DELETE CASCADE;
ALTER TABLE final_grades ADD COLUMN IF NOT EXISTS grade_percentage NUMERIC(5,2) CHECK (grade_percentage IS NULL OR grade_percentage BETWEEN 0 AND 100);
ALTER TABLE final_grades ADD COLUMN IF NOT EXISTS letter_grade VARCHAR(5);
UPDATE final_grades SET grade_percentage = grade WHERE grade_percentage IS NULL AND grade IS NOT NULL;

ALTER TABLE quiz_monitoring_events ADD COLUMN IF NOT EXISTS quiz_attempt_id INT REFERENCES quiz_attempts(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_learning_topics_course_id ON learning_topics (course_id);
CREATE INDEX IF NOT EXISTS idx_course_materials_topic_id ON course_materials (topic_id);
CREATE INDEX IF NOT EXISTS idx_assignments_course_offering_id ON assignments (course_offering_id);
CREATE INDEX IF NOT EXISTS idx_submission_files_submission_id ON submission_files (submission_id);
CREATE INDEX IF NOT EXISTS idx_quizzes_course_offering_id ON quizzes (course_offering_id);
CREATE INDEX IF NOT EXISTS idx_quiz_questions_topic_id ON quiz_questions (topic_id);
CREATE INDEX IF NOT EXISTS idx_student_topic_performance_student_id ON student_topic_performance (student_id);
CREATE INDEX IF NOT EXISTS idx_recommendation_rules_course_id ON recommendation_rules (course_id);
CREATE INDEX IF NOT EXISTS idx_student_recommendations_student_id ON student_recommendations (student_id);
CREATE INDEX IF NOT EXISTS idx_attendance_sessions_course_offering_id ON attendance_sessions (course_offering_id);
CREATE INDEX IF NOT EXISTS idx_attendance_records_student_id ON attendance_records (student_id);
CREATE INDEX IF NOT EXISTS idx_forum_threads_course_offering_id ON forum_threads (course_offering_id);
CREATE INDEX IF NOT EXISTS idx_audit_logs_actor_user_id ON audit_logs (actor_user_id);
