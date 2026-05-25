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
