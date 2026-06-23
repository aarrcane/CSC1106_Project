-- Rollback for sql/attendance_migration.sql.

DROP INDEX IF EXISTS idx_attendance_records_student;
DROP INDEX IF EXISTS idx_attendance_sessions_course_opened;
DROP TABLE IF EXISTS attendance_records;
DROP TABLE IF EXISTS attendance_sessions;
