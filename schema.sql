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

CREATE TABLE IF NOT EXISTS students (
    id SERIAL PRIMARY KEY,
    name VARCHAR(100) NOT NULL,
    email VARCHAR(100) UNIQUE NOT NULL,
    age INT
);

CREATE TABLE IF NOT EXISTS quiz_monitoring_events (
    id SERIAL PRIMARY KEY,
    quiz_id INT NOT NULL,
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

CREATE INDEX IF NOT EXISTS idx_quiz_monitoring_events_quiz_id
    ON quiz_monitoring_events (quiz_id);

CREATE INDEX IF NOT EXISTS idx_quiz_monitoring_events_student_user_id
    ON quiz_monitoring_events (student_user_id);

CREATE INDEX IF NOT EXISTS idx_quiz_monitoring_events_occurred_at
    ON quiz_monitoring_events (occurred_at DESC);

ALTER TABLE quiz_monitoring_events ENABLE ROW LEVEL SECURITY;
