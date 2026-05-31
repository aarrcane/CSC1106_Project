-- CSC1106 LMS database schema
-- Canonical schema for a fresh PostgreSQL/Supabase database.
-- Designed around normalized LMS entities, role-based users, adaptive quizzes,
-- submissions, attendance, discussion forums, recommendations, and audit logs.

CREATE TABLE IF NOT EXISTS users (
    id SERIAL PRIMARY KEY,
    display_name VARCHAR(100) NOT NULL,
    email VARCHAR(255) UNIQUE NOT NULL,
    password_hash TEXT NOT NULL,
    role VARCHAR(20) NOT NULL CHECK (role IN ('student', 'lecturer', 'admin')),
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS students (
    id SERIAL PRIMARY KEY,
    user_id INT NOT NULL UNIQUE REFERENCES users(id) ON DELETE CASCADE,
    student_no VARCHAR(50) NOT NULL UNIQUE,
    programme VARCHAR(100),
    year_of_study INT CHECK (year_of_study IS NULL OR year_of_study BETWEEN 1 AND 10),
    date_of_birth DATE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS lecturers (
    id SERIAL PRIMARY KEY,
    user_id INT NOT NULL UNIQUE REFERENCES users(id) ON DELETE CASCADE,
    staff_no VARCHAR(50) NOT NULL UNIQUE,
    department VARCHAR(100) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

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

CREATE TABLE IF NOT EXISTS courses (
    id SERIAL PRIMARY KEY,
    course_code VARCHAR(20) NOT NULL UNIQUE,
    course_name VARCHAR(100) NOT NULL,
    description TEXT NOT NULL,
    credits INT NOT NULL DEFAULT 15 CHECK (credits > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

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

CREATE TABLE IF NOT EXISTS enrollments (
    id SERIAL PRIMARY KEY,
    student_id INT NOT NULL REFERENCES students(id) ON DELETE CASCADE,
    course_offering_id INT NOT NULL REFERENCES course_offerings(id) ON DELETE CASCADE,
    status VARCHAR(20) NOT NULL DEFAULT 'active'
        CHECK (status IN ('active', 'dropped', 'completed')),
    enrolled_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (student_id, course_offering_id)
);

CREATE TABLE IF NOT EXISTS announcements (
    id SERIAL PRIMARY KEY,
    course_offering_id INT NOT NULL REFERENCES course_offerings(id) ON DELETE CASCADE,
    posted_by INT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    title VARCHAR(200) NOT NULL,
    content TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS learning_topics (
    id SERIAL PRIMARY KEY,
    course_id INT NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
    topic_code VARCHAR(50) NOT NULL,
    title VARCHAR(150) NOT NULL,
    description TEXT,
    display_order INT NOT NULL DEFAULT 0,
    UNIQUE (course_id, topic_code)
);

CREATE TABLE IF NOT EXISTS course_materials (
    id SERIAL PRIMARY KEY,
    course_id INT NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
    topic_id INT REFERENCES learning_topics(id) ON DELETE SET NULL,
    uploaded_by INT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    title VARCHAR(200) NOT NULL,
    description TEXT,
    storage_path VARCHAR(500) NOT NULL,
    original_filename VARCHAR(255),
    mime_type VARCHAR(100),
    file_size_bytes BIGINT CHECK (file_size_bytes IS NULL OR file_size_bytes >= 0),
    material_type VARCHAR(50) NOT NULL
        CHECK (material_type IN ('note', 'slide', 'video', 'link', 'assignment_brief', 'other')),
    uploaded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (course_id, storage_path)
);

CREATE TABLE IF NOT EXISTS assignments (
    id SERIAL PRIMARY KEY,
    course_offering_id INT NOT NULL REFERENCES course_offerings(id) ON DELETE CASCADE,
    created_by INT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    title VARCHAR(200) NOT NULL,
    description TEXT NOT NULL,
    due_at TIMESTAMPTZ NOT NULL,
    max_score NUMERIC(6,2) NOT NULL CHECK (max_score > 0),
    allow_late BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS submissions (
    id SERIAL PRIMARY KEY,
    assignment_id INT NOT NULL REFERENCES assignments(id) ON DELETE CASCADE,
    student_id INT NOT NULL REFERENCES students(id) ON DELETE CASCADE,
    attempt_no INT NOT NULL DEFAULT 1 CHECK (attempt_no > 0),
    submitted_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    status VARCHAR(20) NOT NULL DEFAULT 'submitted'
        CHECK (status IN ('draft', 'submitted', 'late', 'graded', 'returned')),
    grade NUMERIC(6,2) CHECK (grade IS NULL OR grade >= 0),
    feedback TEXT,
    graded_by INT REFERENCES users(id) ON DELETE SET NULL,
    graded_at TIMESTAMPTZ,
    UNIQUE (assignment_id, student_id, attempt_no)
);

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

CREATE TABLE IF NOT EXISTS quizzes (
    id SERIAL PRIMARY KEY,
    course_offering_id INT NOT NULL REFERENCES course_offerings(id) ON DELETE CASCADE,
    created_by INT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    title VARCHAR(200) NOT NULL,
    description TEXT,
    open_at TIMESTAMPTZ NOT NULL,
    close_at TIMESTAMPTZ NOT NULL,
    duration_mins INT NOT NULL CHECK (duration_mins > 0),
    max_attempts INT NOT NULL DEFAULT 1 CHECK (max_attempts > 0),
    total_marks NUMERIC(6,2) NOT NULL CHECK (total_marks > 0),
    adaptive_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    recommendation_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    status VARCHAR(20) NOT NULL DEFAULT 'draft'
        CHECK (status IN ('draft', 'published', 'closed', 'archived')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (close_at > open_at)
);

CREATE TABLE IF NOT EXISTS quiz_questions (
    id SERIAL PRIMARY KEY,
    quiz_id INT NOT NULL REFERENCES quizzes(id) ON DELETE CASCADE,
    topic_id INT REFERENCES learning_topics(id) ON DELETE SET NULL,
    position INT NOT NULL CHECK (position > 0),
    question_text TEXT NOT NULL,
    question_type VARCHAR(20) NOT NULL
        CHECK (question_type IN ('multiple_choice', 'short_answer', 'true_false')),
    difficulty_level INT NOT NULL DEFAULT 1 CHECK (difficulty_level BETWEEN 1 AND 5),
    marks NUMERIC(6,2) NOT NULL CHECK (marks > 0),
    explanation TEXT,
    UNIQUE (quiz_id, position)
);

CREATE TABLE IF NOT EXISTS quiz_options (
    id SERIAL PRIMARY KEY,
    question_id INT NOT NULL REFERENCES quiz_questions(id) ON DELETE CASCADE,
    position INT NOT NULL CHECK (position > 0),
    option_text TEXT NOT NULL,
    is_correct BOOLEAN NOT NULL DEFAULT FALSE,
    UNIQUE (question_id, position),
    UNIQUE (id, question_id)
);

CREATE TABLE IF NOT EXISTS quiz_attempts (
    id SERIAL PRIMARY KEY,
    quiz_id INT NOT NULL REFERENCES quizzes(id) ON DELETE CASCADE,
    student_id INT NOT NULL REFERENCES students(id) ON DELETE CASCADE,
    attempt_no INT NOT NULL CHECK (attempt_no > 0),
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    submitted_at TIMESTAMPTZ,
    status VARCHAR(20) NOT NULL DEFAULT 'in_progress'
        CHECK (status IN ('in_progress', 'submitted', 'expired', 'graded')),
    score NUMERIC(6,2) CHECK (score IS NULL OR score >= 0),
    difficulty_snapshot INT CHECK (difficulty_snapshot IS NULL OR difficulty_snapshot BETWEEN 1 AND 5),
    UNIQUE (quiz_id, student_id, attempt_no),
    CHECK (submitted_at IS NULL OR submitted_at >= started_at)
);

CREATE TABLE IF NOT EXISTS quiz_answers (
    id SERIAL PRIMARY KEY,
    attempt_id INT NOT NULL REFERENCES quiz_attempts(id) ON DELETE CASCADE,
    question_id INT NOT NULL REFERENCES quiz_questions(id) ON DELETE CASCADE,
    selected_option_id INT,
    answer_text TEXT,
    marks_awarded NUMERIC(6,2) CHECK (marks_awarded IS NULL OR marks_awarded >= 0),
    is_correct BOOLEAN,
    answered_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (attempt_id, question_id),
    FOREIGN KEY (selected_option_id)
        REFERENCES quiz_options(id)
        ON DELETE SET NULL
);

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

CREATE TABLE IF NOT EXISTS forum_threads (
    id SERIAL PRIMARY KEY,
    course_offering_id INT NOT NULL REFERENCES course_offerings(id) ON DELETE CASCADE,
    created_by INT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    title VARCHAR(255) NOT NULL,
    body TEXT NOT NULL,
    is_pinned BOOLEAN NOT NULL DEFAULT FALSE,
    is_answered BOOLEAN NOT NULL DEFAULT FALSE,
    view_count INT NOT NULL DEFAULT 0 CHECK (view_count >= 0),
    reply_count INT NOT NULL DEFAULT 0 CHECK (reply_count >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS forum_posts (
    id SERIAL PRIMARY KEY,
    thread_id INT NOT NULL REFERENCES forum_threads(id) ON DELETE CASCADE,
    user_id INT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    body TEXT NOT NULL,
    is_answer BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS forum_tags (
    id SERIAL PRIMARY KEY,
    name VARCHAR(50) NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS forum_thread_tags (
    thread_id INT NOT NULL REFERENCES forum_threads(id) ON DELETE CASCADE,
    tag_id INT NOT NULL REFERENCES forum_tags(id) ON DELETE CASCADE,
    PRIMARY KEY (thread_id, tag_id)
);

CREATE TABLE IF NOT EXISTS notifications (
    id SERIAL PRIMARY KEY,
    user_id INT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    title VARCHAR(200) NOT NULL,
    message TEXT NOT NULL,
    target_url VARCHAR(500),
    is_read BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS final_grades (
    id SERIAL PRIMARY KEY,
    student_id INT NOT NULL REFERENCES students(id) ON DELETE CASCADE,
    course_offering_id INT NOT NULL REFERENCES course_offerings(id) ON DELETE CASCADE,
    grade_percentage NUMERIC(5,2) NOT NULL CHECK (grade_percentage BETWEEN 0 AND 100),
    letter_grade VARCHAR(5),
    released_at TIMESTAMPTZ,
    approved_by INT REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (student_id, course_offering_id)
);

CREATE TABLE IF NOT EXISTS final_grade_history (
    id SERIAL PRIMARY KEY,
    final_grade_id INT NOT NULL REFERENCES final_grades(id) ON DELETE CASCADE,
    previous_grade_percentage NUMERIC(5,2) CHECK (previous_grade_percentage IS NULL OR previous_grade_percentage BETWEEN 0 AND 100),
    new_grade_percentage NUMERIC(5,2) CHECK (new_grade_percentage IS NULL OR new_grade_percentage BETWEEN 0 AND 100),
    changed_by INT REFERENCES users(id) ON DELETE SET NULL,
    changed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    reason TEXT
);

CREATE TABLE IF NOT EXISTS quiz_monitoring_events (
    id SERIAL PRIMARY KEY,
    quiz_id INT NOT NULL REFERENCES quizzes(id) ON DELETE CASCADE,
    quiz_attempt_id INT REFERENCES quiz_attempts(id) ON DELETE SET NULL,
    student_user_id INT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    student_display_name VARCHAR(100) NOT NULL,
    event_type VARCHAR(40) NOT NULL CHECK (
        event_type IN (
            'monitoring_started',
            'monitoring_error',
            'camera_permission_denied',
            'microphone_permission_denied',
            'face_missing',
            'face_restored',
            'multiple_faces',
            'looking_away',
            'noise_spike'
        )
    ),
    severity VARCHAR(20) NOT NULL CHECK (severity IN ('info', 'warning', 'critical')),
    details TEXT CHECK (details IS NULL OR char_length(details) <= 500),
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
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

CREATE INDEX IF NOT EXISTS idx_students_user_id ON students (user_id);
CREATE INDEX IF NOT EXISTS idx_lecturers_user_id ON lecturers (user_id);
CREATE INDEX IF NOT EXISTS idx_course_offerings_course_id ON course_offerings (course_id);
CREATE INDEX IF NOT EXISTS idx_course_offerings_lecturer_id ON course_offerings (lecturer_id);
CREATE INDEX IF NOT EXISTS idx_enrollments_student_id ON enrollments (student_id);
CREATE INDEX IF NOT EXISTS idx_enrollments_course_offering_id ON enrollments (course_offering_id);
CREATE INDEX IF NOT EXISTS idx_announcements_course_offering_id ON announcements (course_offering_id);
CREATE INDEX IF NOT EXISTS idx_learning_topics_course_id ON learning_topics (course_id);
CREATE INDEX IF NOT EXISTS idx_course_materials_course_id ON course_materials (course_id);
CREATE INDEX IF NOT EXISTS idx_course_materials_topic_id ON course_materials (topic_id);
CREATE INDEX IF NOT EXISTS idx_assignments_course_offering_id ON assignments (course_offering_id);
CREATE INDEX IF NOT EXISTS idx_submissions_assignment_student ON submissions (assignment_id, student_id);
CREATE INDEX IF NOT EXISTS idx_submission_files_submission_id ON submission_files (submission_id);
CREATE INDEX IF NOT EXISTS idx_quizzes_course_offering_id ON quizzes (course_offering_id);
CREATE INDEX IF NOT EXISTS idx_quiz_questions_quiz_id ON quiz_questions (quiz_id);
CREATE INDEX IF NOT EXISTS idx_quiz_questions_topic_id ON quiz_questions (topic_id);
CREATE INDEX IF NOT EXISTS idx_quiz_options_question_id ON quiz_options (question_id);
CREATE INDEX IF NOT EXISTS idx_quiz_attempts_quiz_student ON quiz_attempts (quiz_id, student_id);
CREATE INDEX IF NOT EXISTS idx_quiz_answers_attempt_id ON quiz_answers (attempt_id);
CREATE INDEX IF NOT EXISTS idx_student_topic_performance_student_id ON student_topic_performance (student_id);
CREATE INDEX IF NOT EXISTS idx_student_topic_performance_topic_id ON student_topic_performance (topic_id);
CREATE INDEX IF NOT EXISTS idx_recommendation_rules_course_id ON recommendation_rules (course_id);
CREATE INDEX IF NOT EXISTS idx_student_recommendations_student_id ON student_recommendations (student_id);
CREATE INDEX IF NOT EXISTS idx_attendance_sessions_course_offering_id ON attendance_sessions (course_offering_id);
CREATE INDEX IF NOT EXISTS idx_attendance_records_student_id ON attendance_records (student_id);
CREATE INDEX IF NOT EXISTS idx_forum_threads_course_offering_id ON forum_threads (course_offering_id);
CREATE INDEX IF NOT EXISTS idx_forum_posts_thread_id ON forum_posts (thread_id);
CREATE INDEX IF NOT EXISTS idx_notifications_user_id ON notifications (user_id);
CREATE INDEX IF NOT EXISTS idx_final_grades_course_student ON final_grades (course_offering_id, student_id);
CREATE INDEX IF NOT EXISTS idx_final_grade_history_final_grade_id ON final_grade_history (final_grade_id);
CREATE INDEX IF NOT EXISTS idx_quiz_monitoring_events_quiz_id ON quiz_monitoring_events (quiz_id);
CREATE INDEX IF NOT EXISTS idx_quiz_monitoring_events_student_user_id ON quiz_monitoring_events (student_user_id);
CREATE INDEX IF NOT EXISTS idx_quiz_monitoring_events_occurred_at ON quiz_monitoring_events (occurred_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_logs_actor_user_id ON audit_logs (actor_user_id);
CREATE INDEX IF NOT EXISTS idx_audit_logs_entity ON audit_logs (entity_type, entity_id);

ALTER TABLE quiz_monitoring_events ENABLE ROW LEVEL SECURITY;

INSERT INTO users (display_name, email, password_hash, role, is_active)
VALUES
    (
        'Demo Student',
        'student@lms.test',
        '$argon2id$v=19$m=19456,t=2,p=1$c3R1ZGVudC1sb2dpbi1zYWx0$YjXh5UzX4GL4BL/LSooMhnKaT0MjIE+cdvu5wQg3Yk0',
        'student',
        TRUE
    ),
    (
        'Demo Lecturer',
        'lecturer@lms.test',
        '$argon2id$v=19$m=19456,t=2,p=1$bGVjdHVyZXItbG9naW4tc2FsdA$vhmdsUYE/totFzciDMtHPOUP6q1oEfuHbrKgjiQkz/k',
        'lecturer',
        TRUE
    ),
    (
        'Demo Admin',
        'admin@lms.test',
        '$argon2id$v=19$m=19456,t=2,p=1$YWRtaW4tbG9naW4tc2FsdA$f766sAwxGI+3KaoHnIDTHmBD84IkMi3vn9M7wkkIzC4',
        'admin',
        TRUE
    )
ON CONFLICT (email) DO UPDATE
SET
    display_name = EXCLUDED.display_name,
    password_hash = EXCLUDED.password_hash,
    role = EXCLUDED.role,
    is_active = EXCLUDED.is_active,
    updated_at = NOW();

INSERT INTO students (user_id, student_no, programme, year_of_study)
SELECT id, '2501129', 'BSc Computing Science', 1
FROM users
WHERE email = 'student@lms.test'
ON CONFLICT (user_id) DO UPDATE
SET
    student_no = EXCLUDED.student_no,
    programme = EXCLUDED.programme,
    year_of_study = EXCLUDED.year_of_study,
    updated_at = NOW();

INSERT INTO lecturers (user_id, staff_no, department)
SELECT id, 'L0001', 'School of Computing Science'
FROM users
WHERE email = 'lecturer@lms.test'
ON CONFLICT (user_id) DO UPDATE
SET
    staff_no = EXCLUDED.staff_no,
    department = EXCLUDED.department,
    updated_at = NOW();

INSERT INTO admins (user_id, staff_no)
SELECT id, 'A0001'
FROM users
WHERE email = 'admin@lms.test'
ON CONFLICT (user_id) DO UPDATE
SET staff_no = EXCLUDED.staff_no;

INSERT INTO academic_terms (term_code, name, starts_on, ends_on)
VALUES ('2025-T3', '2025/26 Trimester 3', DATE '2026-03-01', DATE '2026-06-30')
ON CONFLICT (term_code) DO UPDATE
SET
    name = EXCLUDED.name,
    starts_on = EXCLUDED.starts_on,
    ends_on = EXCLUDED.ends_on;

INSERT INTO courses (course_code, course_name, description, credits)
VALUES
    ('CSC1106', 'Web Programming', 'Rust, Actix Web, server-side rendering, database-backed LMS workflows.', 15),
    ('CSC1107', 'Operating Systems', 'Operating system concepts, processes, memory, and file systems.', 15),
    ('INF2003', 'Database Systems', 'Relational database design, SQL, normalization, and transactions.', 15)
ON CONFLICT (course_code) DO UPDATE
SET
    course_name = EXCLUDED.course_name,
    description = EXCLUDED.description,
    credits = EXCLUDED.credits,
    updated_at = NOW();

INSERT INTO course_offerings (course_id, term_id, lecturer_id, section, capacity, status)
SELECT c.id, t.id, l.id, 'A', 50, 'ongoing'
FROM courses c
JOIN academic_terms t ON t.term_code = '2025-T3'
JOIN lecturers l ON l.staff_no = 'L0001'
WHERE c.course_code = 'CSC1106'
ON CONFLICT (course_id, term_id, section) DO UPDATE
SET
    lecturer_id = EXCLUDED.lecturer_id,
    capacity = EXCLUDED.capacity,
    status = EXCLUDED.status,
    updated_at = NOW();

INSERT INTO course_offerings (course_id, term_id, lecturer_id, section, capacity, status)
SELECT c.id, t.id, l.id, 'A', 45, 'ongoing'
FROM courses c
JOIN academic_terms t ON t.term_code = '2025-T3'
JOIN lecturers l ON l.staff_no = 'L0001'
WHERE c.course_code IN ('CSC1107', 'INF2003')
ON CONFLICT (course_id, term_id, section) DO UPDATE
SET
    lecturer_id = EXCLUDED.lecturer_id,
    capacity = EXCLUDED.capacity,
    status = EXCLUDED.status,
    updated_at = NOW();

INSERT INTO enrollments (student_id, course_offering_id, status)
SELECT s.id, co.id, 'active'
FROM students s
JOIN users u ON u.id = s.user_id
JOIN course_offerings co ON TRUE
JOIN courses c ON c.id = co.course_id
JOIN academic_terms t ON t.id = co.term_id
WHERE u.email = 'student@lms.test'
    AND c.course_code = 'CSC1106'
    AND t.term_code = '2025-T3'
    AND co.section = 'A'
ON CONFLICT (student_id, course_offering_id) DO UPDATE
SET status = EXCLUDED.status;

INSERT INTO learning_topics (course_id, topic_code, title, description, display_order)
SELECT c.id, topic_code, title, description, display_order
FROM courses c
CROSS JOIN (
    VALUES
        ('JS-VAR', 'JavaScript Variables', 'Variable declarations, scope, and mutability.', 1),
        ('FETCH', 'Fetch API', 'Asynchronous JSON requests in browser applications.', 2),
        ('DOM', 'Document Object Model', 'DOM structure, traversal, and updates.', 3)
) AS seed(topic_code, title, description, display_order)
WHERE c.course_code = 'CSC1106'
ON CONFLICT (course_id, topic_code) DO UPDATE
SET
    title = EXCLUDED.title,
    description = EXCLUDED.description,
    display_order = EXCLUDED.display_order;

INSERT INTO course_materials (course_id, topic_id, uploaded_by, title, description, storage_path, original_filename, mime_type, material_type)
SELECT c.id, lt.id, u.id, 'JavaScript Fundamentals Notes', 'Revision notes for variables, Fetch API, and DOM basics.', '/materials/csc1106/js-fundamentals.pdf', 'js-fundamentals.pdf', 'application/pdf', 'note'
FROM courses c
JOIN learning_topics lt ON lt.course_id = c.id AND lt.topic_code = 'JS-VAR'
JOIN users u ON u.email = 'lecturer@lms.test'
WHERE c.course_code = 'CSC1106'
ON CONFLICT DO NOTHING;

INSERT INTO quizzes (id, course_offering_id, created_by, title, description, open_at, close_at, duration_mins, max_attempts, total_marks, adaptive_enabled, recommendation_enabled, status)
SELECT
    2,
    co.id,
    u.id,
    'Quiz 2 - JavaScript Fundamentals',
    'Demo quiz used for the student quiz attempt and monitoring workflow.',
    TIMESTAMPTZ '2026-05-01 00:00:00+08',
    TIMESTAMPTZ '2026-06-30 23:59:00+08',
    25,
    2,
    25,
    TRUE,
    TRUE,
    'published'
FROM course_offerings co
JOIN courses c ON c.id = co.course_id
JOIN academic_terms t ON t.id = co.term_id
JOIN users u ON u.email = 'lecturer@lms.test'
WHERE c.course_code = 'CSC1106'
    AND t.term_code = '2025-T3'
    AND co.section = 'A'
ON CONFLICT (id) DO UPDATE
SET
    course_offering_id = EXCLUDED.course_offering_id,
    created_by = EXCLUDED.created_by,
    title = EXCLUDED.title,
    description = EXCLUDED.description,
    open_at = EXCLUDED.open_at,
    close_at = EXCLUDED.close_at,
    duration_mins = EXCLUDED.duration_mins,
    max_attempts = EXCLUDED.max_attempts,
    total_marks = EXCLUDED.total_marks,
    adaptive_enabled = EXCLUDED.adaptive_enabled,
    recommendation_enabled = EXCLUDED.recommendation_enabled,
    status = EXCLUDED.status,
    updated_at = NOW();

SELECT setval(pg_get_serial_sequence('quizzes', 'id'), GREATEST((SELECT MAX(id) FROM quizzes), 1), TRUE);

INSERT INTO quiz_questions (quiz_id, topic_id, position, question_text, question_type, difficulty_level, marks, explanation)
SELECT q.id, lt.id, seed.position, seed.question_text, 'multiple_choice', seed.difficulty_level, seed.marks, seed.explanation
FROM quizzes q
JOIN course_offerings co ON co.id = q.course_offering_id
JOIN courses c ON c.id = co.course_id
JOIN (
    VALUES
        (1, 'JS-VAR', 'Which keyword declares a block-scoped JavaScript variable?', 1, 5, 'let and const are block-scoped.'),
        (2, 'FETCH', 'Which browser API is commonly used to request JSON data asynchronously?', 2, 5, 'The Fetch API is commonly used for async HTTP requests.'),
        (3, 'DOM', 'What does DOM stand for?', 1, 5, 'DOM means Document Object Model.')
) AS seed(position, seed_topic, question_text, difficulty_level, marks, explanation)
    ON TRUE
JOIN learning_topics lt ON lt.course_id = c.id AND lt.topic_code = seed.seed_topic
WHERE q.id = 2
ON CONFLICT (quiz_id, position) DO UPDATE
SET
    topic_id = EXCLUDED.topic_id,
    question_text = EXCLUDED.question_text,
    question_type = EXCLUDED.question_type,
    difficulty_level = EXCLUDED.difficulty_level,
    marks = EXCLUDED.marks,
    explanation = EXCLUDED.explanation;

INSERT INTO quiz_options (question_id, position, option_text, is_correct)
SELECT qq.id, seed.position, seed.option_text, seed.is_correct
FROM quiz_questions qq
JOIN quizzes q ON q.id = qq.quiz_id
JOIN (
    VALUES
        (1, 1, 'var', FALSE),
        (1, 2, 'let', TRUE),
        (1, 3, 'static', FALSE),
        (1, 4, 'global', FALSE),
        (2, 1, 'Fetch API', TRUE),
        (2, 2, 'Canvas API', FALSE),
        (2, 3, 'Storage API', FALSE),
        (2, 4, 'History API', FALSE),
        (3, 1, 'Document Object Model', TRUE),
        (3, 2, 'Data Object Map', FALSE),
        (3, 3, 'Display Output Method', FALSE),
        (3, 4, 'Document Order Mode', FALSE)
) AS seed(question_position, position, option_text, is_correct)
    ON seed.question_position = qq.position
WHERE q.id = 2
ON CONFLICT (question_id, position) DO UPDATE
SET
    option_text = EXCLUDED.option_text,
    is_correct = EXCLUDED.is_correct;
