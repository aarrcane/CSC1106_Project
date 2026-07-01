# CSC1106_Project

A small LMS-style web application written in Rust using Actix-web, sqlx (Postgres), and Tera templates.

Quick start

1. Create a `.env` file in the project root with your Supabase connection details and a session secret:

```env
DATABASE_URL=postgresql://postgres.PROJECT_REF:YOUR_DB_PASSWORD@aws-1-ap-northeast-2.pooler.supabase.com:5432/postgres?sslmode=require
SESSION_SECRET=01234567890123456789012345678901
```

Use the Supabase Session Pooler URL, not the Transaction Pooler URL.

2. Run the server:

```bash
cargo run
```

3. Open http://127.0.0.1:8080/ in your browser.

- Admin: `admin@lms.test` / `Password123!`

Notes

- Database migrations and seed scripts are stored in the `sql/` directory.
- Templates live in `templates/` and static assets in `static/`.
- Main server code and shared helpers: `src/main.rs`; role-specific handlers are in `src/admin.rs`, `src/student.rs`, and `src/lecturer.rs`.
- Authentication helpers are in `src/auth.rs`.

Development

- Use `cargo check` to verify compilation quickly.
- Ensure your `DATABASE_URL` points to the shared Supabase database before running the app locally.
