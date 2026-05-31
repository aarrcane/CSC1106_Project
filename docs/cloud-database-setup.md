# Cloud Database Setup For Groupmates

The shared Supabase PostgreSQL database has already been created and the schema has already been applied. You do not need to create a new Supabase project or run `schema.sql` again unless the group agrees to reset/update the database.

## What You Need

Ask YH privately for the shared Supabase `DATABASE_URL`. Do not post the real URL or password in GitHub, Teams, Discord, or any public chat.

The URL should look similar to this:

```env
DATABASE_URL=postgresql://postgres.PROJECT_REF:YOUR_DB_PASSWORD@aws-1-ap-northeast-2.pooler.supabase.com:5432/postgres?sslmode=require
```

Important:

- Use the Supabase **Session Pooler** URL.
- Keep `?sslmode=require` at the end.
- Do not use the Transaction Pooler URL for this Actix app.
- Do not commit your `.env` file.

## Local Setup

1. Copy `.env.example` and rename the copy to `.env`.
2. Replace the example `DATABASE_URL` with the shared Supabase URL from YH.
3. Set your own `SESSION_SECRET`.

Example:

```env
DATABASE_URL=postgresql://postgres.PROJECT_REF:YOUR_DB_PASSWORD@aws-1-ap-northeast-2.pooler.supabase.com:5432/postgres?sslmode=require
SESSION_SECRET=replace-this-with-your-own-64-character-minimum-secret
```

The `SESSION_SECRET` can be different for each teammate. It only needs to be private and at least 64 characters long.

If the database password contains special characters such as `@`, `:`, `/`, or `?`, those characters may need to be URL-encoded inside `DATABASE_URL`.

## Verify It Works

Run:

```powershell
cargo check
cargo run
```

Then open:

```text
http://127.0.0.1:8080/
```

Test login with the demo accounts:

```text
student@lms.test
lecturer@lms.test
admin@lms.test
```

Demo password:

```text
Password123!
```

If login works, your local website is using the shared Supabase database.

## Schema Updates

The shared database is used by the whole group. Do not rerun `schema.sql`, delete tables, or reset data unless the team has agreed.

If the group agrees to update the current shared Supabase database to the normalized LMS schema, apply this file once in the Supabase SQL Editor:

```text
sql/2026-05-31_lms_schema_alignment.sql
```

For a brand new or reset database, apply the root `schema.sql` instead.

## References

- Supabase database connections: https://supabase.com/docs/guides/database/connecting-to-postgres
- Supabase migrations and team workflow: https://supabase.com/docs/guides/deployment/database-migrations
- Supabase Storage for later submissions: https://supabase.com/docs/guides/storage
- SQLx prepared statements: https://docs.rs/sqlx/latest/sqlx/
