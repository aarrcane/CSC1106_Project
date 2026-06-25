-- One-time migration for the attendance check-in feature.

CREATE TABLE IF NOT EXISTS attendance_sessions (
    id SERIAL PRIMARY KEY,
    course_id INT NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
    created_by INT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    session_title VARCHAR(200) NOT NULL,
    check_in_code VARCHAR(12) UNIQUE NOT NULL,
    status VARCHAR(20) NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'closed')),
    late_after_minutes INT NOT NULL DEFAULT 10 CHECK (late_after_minutes >= 0),
    opened_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    closed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS attendance_records (
    id SERIAL PRIMARY KEY,
    session_id INT NOT NULL REFERENCES attendance_sessions(id) ON DELETE CASCADE,
    student_id INT NOT NULL REFERENCES students(id) ON DELETE CASCADE,
    status VARCHAR(20) NOT NULL DEFAULT 'absent' CHECK (status IN ('present', 'late', 'absent', 'excused')),
    checked_in_at TIMESTAMPTZ,
    marked_by INT REFERENCES users(id) ON DELETE SET NULL,
    note TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (session_id, student_id)
);

CREATE INDEX IF NOT EXISTS idx_attendance_sessions_course_opened
    ON attendance_sessions(course_id, opened_at DESC);

CREATE INDEX IF NOT EXISTS idx_attendance_records_student
    ON attendance_records(student_id, session_id);
