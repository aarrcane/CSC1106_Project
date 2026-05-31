# Database Schema Notes

The canonical schema is `schema.sql`. For a clean development database, reset the database and apply `schema.sql`.

For the existing shared Supabase database, apply:

```text
sql/2026-05-31_lms_schema_alignment.sql
```

This is additive where possible so prototype data is not intentionally dropped.

## Main Design Choices

- `users` stores login identity, password hash, role, email, and display name.
- `students`, `lecturers`, and `admins` store role-specific profile fields only.
- `courses` stores the course definition, while `course_offerings` stores trimester, lecturer, capacity, and status.
- `enrollments` links students to course offerings, not directly to course definitions.
- Assignment submissions are split into `submissions` and `submission_files`.
- Forum tags are normalized into `forum_tags` and `forum_thread_tags`.
- Attendance has `attendance_sessions` and `attendance_records`.
- Quiz adaptation is supported through `learning_topics`, `difficulty_level`, `student_topic_performance`, `recommendation_rules`, and `student_recommendations`.
- `quiz_monitoring_events` logs suspicious signals only; it does not store video, images, or audio.

## Implementation Priority

When replacing hardcoded pages with database queries, start with:

1. User profile lookup from `users` + `students` / `lecturers`.
2. Course lists from `enrollments` + `course_offerings` + `courses`.
3. Quizzes from `quizzes`, `quiz_questions`, `quiz_options`, and `quiz_attempts`.
4. Assignment submissions from `assignments`, `submissions`, and `submission_files`.
5. Attendance from `attendance_sessions` and `attendance_records`.
6. Recommendations from topic performance and recommendation rules.
