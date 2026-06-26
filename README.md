# CSC1106_Project

A small LMS-style web application written in Rust using Actix-web, sqlx (Postgres), and Tera templates.

Quick start

1. Create a `.env` in the project root with at least the `DATABASE_URL` and optional `SESSION_SECRET`:

	DATABASE_URL=postgres://user:password@localhost:5432/dbname
	SESSION_SECRET=01234567890123456789012345678901

2. Run the server:

```bash
cargo run
```

3. Open http://127.0.0.1:8080/ in your browser.

Notes

- Database migrations are stored in the `sql/` directory. 
- Templates live in `templates/` and static assets in `static/`.
- Main server code and shared helpers: `src/main.rs`; role-specific handlers are in `src/admin.rs`, `src/student.rs`, and `src/lecturer.rs`.
- Authentication helpers are in `src/auth.rs`.

Development

- Use `cargo check` to verify compilation quickly.
- To run the app locally ensure Postgres is running and the `DATABASE_URL` points to a reachable database.
