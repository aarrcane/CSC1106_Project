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
