use actix_session::Session;
use actix_web::{HttpResponse, http::header, web};
use argon2::password_hash::rand_core::{OsRng, RngCore};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use tera::{Context, Tera};

use crate::auth::UserRole;

const LATE_AFTER_MINUTES: i32 = 10;
const FLASH_SUCCESS: &str = "attendance_success";
const FLASH_ERROR: &str = "attendance_error";

#[derive(Deserialize)]
pub struct CreateSessionForm {
    course_id: i32,
    session_title: String,
}

#[derive(Deserialize)]
pub struct CheckInForm {
    code: String,
}

#[derive(Deserialize)]
pub struct UpdateRecordForm {
    status: String,
    note: Option<String>,
}

#[derive(Serialize, FromRow)]
struct CourseOption {
    id: i32,
    code: String,
    name: String,
}

#[derive(Serialize, FromRow)]
struct LecturerSessionRow {
    id: i32,
    course_id: i32,
    course_code: String,
    course_name: String,
    session_title: String,
    check_in_code: String,
    status: String,
    late_after_minutes: i32,
    opened_at: String,
    closed_at: Option<String>,
    total_count: i64,
    present_count: i64,
    late_count: i64,
    absent_count: i64,
    excused_count: i64,
}

#[derive(Serialize)]
struct LecturerSessionView {
    id: i32,
    course_id: i32,
    course_code: String,
    course_name: String,
    session_title: String,
    check_in_code: String,
    status: String,
    late_after_minutes: i32,
    opened_at: String,
    closed_at: Option<String>,
    total_count: i64,
    present_count: i64,
    late_count: i64,
    absent_count: i64,
    excused_count: i64,
    records: Vec<LecturerRecordView>,
}

#[derive(Serialize, FromRow)]
struct LecturerRecordView {
    id: i32,
    student_name: String,
    student_email: String,
    status: String,
    checked_in_at: Option<String>,
    note: String,
}

#[derive(Serialize, FromRow)]
struct LecturerCourseOverview {
    course_id: i32,
    course_code: String,
    course_name: String,
    enrolled_count: i64,
    total_sessions: i64,
    total_records: i64,
    attendance_pct: i32,
    present_count: i64,
    late_count: i64,
    absent_count: i64,
    excused_count: i64,
    latest_session_title: Option<String>,
    latest_session_status: Option<String>,
    latest_session_opened_at: Option<String>,
}

#[derive(Serialize)]
struct StudentAttendanceCourseView {
    id: i32,
    code: String,
    name: String,
    pct: i32,
    attended: i32,
    total: i32,
    sessions: Vec<StudentAttendanceSessionView>,
}

#[derive(Serialize, FromRow)]
struct StudentAttendanceSessionView {
    date: String,
    topic: String,
    status: String,
}

#[derive(Serialize, FromRow)]
struct StudentActiveSessionView {
    id: i32,
    course_code: String,
    course_name: String,
    session_title: String,
    late_after_minutes: i32,
    opened_at: String,
    checked_in_status: Option<String>,
    checked_in_at: Option<String>,
}

#[derive(FromRow)]
struct StudentProfile {
    id: i32,
}

#[derive(FromRow)]
struct LecturerProfile {
    id: i32,
}

#[derive(FromRow)]
struct SessionCheck {
    id: i32,
    status: String,
    opened_at: chrono::DateTime<Utc>,
    late_after_minutes: i32,
}

#[derive(FromRow)]
struct IdRow {
    id: i32,
}

pub async fn lecturer_attendance(
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
) -> HttpResponse {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };
    let lecturer = match lecturer_profile(db.get_ref(), user.id).await {
        Ok(Some(lecturer)) => lecturer,
        Ok(None) => return HttpResponse::Forbidden().body("No lecturer profile found."),
        Err(error) => return HttpResponse::InternalServerError().body(error.to_string()),
    };

    let course_overviews = match lecturer_course_overviews(db.get_ref(), lecturer.id).await {
        Ok(overviews) => overviews,
        Err(error) => return HttpResponse::InternalServerError().body(error.to_string()),
    };

    let mut ctx = Context::new();
    insert_lecturer_base(&mut ctx, &user.display_name);
    take_flash(&session, &mut ctx);
    ctx.insert("course_overviews", &course_overviews);

    render(&tmpl, "lecturer/attendance_overview.html", &ctx)
}

pub async fn lecturer_attendance_sessions(
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
) -> HttpResponse {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };
    let lecturer = match lecturer_profile(db.get_ref(), user.id).await {
        Ok(Some(lecturer)) => lecturer,
        Ok(None) => return HttpResponse::Forbidden().body("No lecturer profile found."),
        Err(error) => return HttpResponse::InternalServerError().body(error.to_string()),
    };

    let courses = match lecturer_courses(db.get_ref(), lecturer.id).await {
        Ok(courses) => courses,
        Err(error) => return HttpResponse::InternalServerError().body(error.to_string()),
    };
    let sessions = match lecturer_sessions(db.get_ref(), lecturer.id).await {
        Ok(sessions) => sessions,
        Err(error) => return HttpResponse::InternalServerError().body(error.to_string()),
    };

    let mut ctx = Context::new();
    insert_lecturer_base(&mut ctx, &user.display_name);
    take_flash(&session, &mut ctx);
    ctx.insert("courses", &courses);
    ctx.insert("sessions", &sessions);
    ctx.insert("late_after_minutes", &LATE_AFTER_MINUTES);

    render(&tmpl, "lecturer/attendance.html", &ctx)
}

pub async fn lecturer_attendance_session_detail(
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
    path: web::Path<i32>,
) -> HttpResponse {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };
    let lecturer = match lecturer_profile(db.get_ref(), user.id).await {
        Ok(Some(lecturer)) => lecturer,
        Ok(None) => return HttpResponse::Forbidden().body("No lecturer profile found."),
        Err(error) => return HttpResponse::InternalServerError().body(error.to_string()),
    };
    let session_id = path.into_inner();

    let session_view = match lecturer_session(db.get_ref(), lecturer.id, session_id).await {
        Ok(Some(session_view)) => session_view,
        Ok(None) => return HttpResponse::NotFound().body("Attendance session not found."),
        Err(error) => return HttpResponse::InternalServerError().body(error.to_string()),
    };

    let mut ctx = Context::new();
    insert_lecturer_base(&mut ctx, &user.display_name);
    take_flash(&session, &mut ctx);
    ctx.insert("session", &session_view);

    render(&tmpl, "lecturer/attendance_session_detail.html", &ctx)
}

pub async fn create_session(
    db: web::Data<PgPool>,
    session: Session,
    form: web::Form<CreateSessionForm>,
) -> HttpResponse {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };
    let lecturer = match lecturer_profile(db.get_ref(), user.id).await {
        Ok(Some(lecturer)) => lecturer,
        Ok(None) => return HttpResponse::Forbidden().body("No lecturer profile found."),
        Err(error) => return HttpResponse::InternalServerError().body(error.to_string()),
    };

    if form.session_title.trim().is_empty() {
        set_flash(&session, FLASH_ERROR, "Session title is required.");
        return redirect("/lecturer/attendance/sessions");
    }

    match lecturer_owns_course(db.get_ref(), lecturer.id, form.course_id).await {
        Ok(true) => {}
        Ok(false) => {
            return HttpResponse::Forbidden()
                .body("You can only open attendance for your own courses.");
        }
        Err(error) => return HttpResponse::InternalServerError().body(error.to_string()),
    }

    let code = generate_check_in_code();
    let mut tx = match db.begin().await {
        Ok(tx) => tx,
        Err(error) => return HttpResponse::InternalServerError().body(error.to_string()),
    };

    let inserted = sqlx::query_as::<_, IdRow>(
        "INSERT INTO attendance_sessions
            (course_id, created_by, session_title, check_in_code, late_after_minutes)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id",
    )
    .bind(form.course_id)
    .bind(user.id)
    .bind(form.session_title.trim())
    .bind(&code)
    .bind(LATE_AFTER_MINUTES)
    .fetch_one(&mut *tx)
    .await;

    let session_row = match inserted {
        Ok(row) => row,
        Err(error) => {
            let _ = tx.rollback().await;
            return HttpResponse::InternalServerError().body(error.to_string());
        }
    };

    let records_result = sqlx::query(
        "INSERT INTO attendance_records (session_id, student_id, status)
         SELECT $1, e.student_id, 'absent'
         FROM enrollments e
         WHERE e.course_id = $2
         ON CONFLICT (session_id, student_id) DO NOTHING",
    )
    .bind(session_row.id)
    .bind(form.course_id)
    .execute(&mut *tx)
    .await;

    if let Err(error) = records_result {
        let _ = tx.rollback().await;
        return HttpResponse::InternalServerError().body(error.to_string());
    }

    if let Err(error) = tx.commit().await {
        return HttpResponse::InternalServerError().body(error.to_string());
    }

    set_flash(
        &session,
        FLASH_SUCCESS,
        &format!("Attendance session opened. Check-in code: {code}"),
    );
    redirect("/lecturer/attendance/sessions")
}

pub async fn close_session(
    db: web::Data<PgPool>,
    session: Session,
    path: web::Path<i32>,
) -> HttpResponse {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };
    let session_id = path.into_inner();

    match lecturer_can_manage_session(db.get_ref(), user.id, session_id).await {
        Ok(true) => {}
        Ok(false) => {
            return HttpResponse::Forbidden()
                .body("You can only close your own attendance sessions.");
        }
        Err(error) => return HttpResponse::InternalServerError().body(error.to_string()),
    }

    match sqlx::query(
        "UPDATE attendance_sessions
         SET status = 'closed', closed_at = COALESCE(closed_at, NOW()), updated_at = NOW()
         WHERE id = $1",
    )
    .bind(session_id)
    .execute(db.get_ref())
    .await
    {
        Ok(_) => {
            set_flash(&session, FLASH_SUCCESS, "Attendance session closed.");
            redirect("/lecturer/attendance/sessions")
        }
        Err(error) => HttpResponse::InternalServerError().body(error.to_string()),
    }
}

pub async fn delete_session(
    db: web::Data<PgPool>,
    session: Session,
    path: web::Path<i32>,
) -> HttpResponse {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };
    let session_id = path.into_inner();

    match lecturer_can_manage_session(db.get_ref(), user.id, session_id).await {
        Ok(true) => {}
        Ok(false) => {
            return HttpResponse::Forbidden()
                .body("You can only delete your own attendance sessions.");
        }
        Err(error) => return HttpResponse::InternalServerError().body(error.to_string()),
    }

    match sqlx::query("DELETE FROM attendance_sessions WHERE id = $1")
        .bind(session_id)
        .execute(db.get_ref())
        .await
    {
        Ok(_) => {
            set_flash(&session, FLASH_SUCCESS, "Attendance session deleted.");
            redirect("/lecturer/attendance/sessions")
        }
        Err(error) => HttpResponse::InternalServerError().body(error.to_string()),
    }
}

pub async fn update_record(
    db: web::Data<PgPool>,
    session: Session,
    path: web::Path<i32>,
    form: web::Form<UpdateRecordForm>,
) -> HttpResponse {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };
    let record_id = path.into_inner();
    let status = form.status.trim();
    if !matches!(status, "present" | "late" | "absent" | "excused") {
        return HttpResponse::BadRequest().body("Invalid attendance status.");
    }

    let record_session_id =
        match manageable_record_session_id(db.get_ref(), user.id, record_id).await {
            Ok(Some(session_id)) => session_id,
            Ok(None) => {
                return HttpResponse::Forbidden()
                    .body("You can only edit records for your own courses.");
            }
            Err(error) => return HttpResponse::InternalServerError().body(error.to_string()),
        };

    let note = form.note.as_deref().unwrap_or("").trim();
    match sqlx::query(
        "UPDATE attendance_records
         SET status = $1,
             note = NULLIF($2, ''),
             marked_by = $3,
             checked_in_at = CASE
                 WHEN $1 IN ('present', 'late') AND checked_in_at IS NULL THEN NOW()
                 WHEN $1 IN ('absent', 'excused') THEN checked_in_at
                 ELSE checked_in_at
             END,
             updated_at = NOW()
         WHERE id = $4",
    )
    .bind(status)
    .bind(note)
    .bind(user.id)
    .bind(record_id)
    .execute(db.get_ref())
    .await
    {
        Ok(_) => {
            set_flash(&session, FLASH_SUCCESS, "Attendance record updated.");
            redirect(&format!(
                "/lecturer/attendance/sessions/{record_session_id}"
            ))
        }
        Err(error) => HttpResponse::InternalServerError().body(error.to_string()),
    }
}

pub async fn student_attendance(
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
) -> HttpResponse {
    let user = match crate::auth::require_role(&session, UserRole::Student) {
        Ok(user) => user,
        Err(response) => return response,
    };
    let student = match student_profile(db.get_ref(), user.id).await {
        Ok(Some(student)) => student,
        Ok(None) => return HttpResponse::Forbidden().body("No student profile found."),
        Err(error) => return HttpResponse::InternalServerError().body(error.to_string()),
    };

    let courses = match student_courses(db.get_ref(), student.id).await {
        Ok(courses) => courses,
        Err(error) => return HttpResponse::InternalServerError().body(error.to_string()),
    };
    let attendance_courses =
        match student_attendance_courses(db.get_ref(), student.id, &courses).await {
            Ok(courses) => courses,
            Err(error) => return HttpResponse::InternalServerError().body(error.to_string()),
        };
    let active_sessions = match student_active_sessions(db.get_ref(), student.id).await {
        Ok(sessions) => sessions,
        Err(error) => return HttpResponse::InternalServerError().body(error.to_string()),
    };

    let total_sessions: i32 = attendance_courses.iter().map(|course| course.total).sum();
    let attended_sessions: i32 = attendance_courses
        .iter()
        .map(|course| course.attended)
        .sum();
    let absent_sessions: i32 = attendance_courses
        .iter()
        .flat_map(|course| &course.sessions)
        .filter(|record| record.status == "absent")
        .count() as i32;
    let overall_pct = if total_sessions > 0 {
        (attended_sessions * 100) / total_sessions
    } else {
        0
    };

    let mut ctx = Context::new();
    crate::insert_student_base(&mut ctx, &user.display_name, &student.id.to_string());
    ctx.insert("active_page", "attendance");
    take_flash(&session, &mut ctx);
    ctx.insert("courses", &courses);
    ctx.insert("active_sessions", &active_sessions);
    ctx.insert("attendance_courses", &attendance_courses);
    ctx.insert("total_sessions", &total_sessions);
    ctx.insert("attended_sessions", &attended_sessions);
    ctx.insert("absent_sessions", &absent_sessions);
    ctx.insert("overall_pct", &overall_pct);

    render(&tmpl, "student/attendance.html", &ctx)
}

pub async fn student_check_in(
    db: web::Data<PgPool>,
    session: Session,
    form: web::Form<CheckInForm>,
) -> HttpResponse {
    let user = match crate::auth::require_role(&session, UserRole::Student) {
        Ok(user) => user,
        Err(response) => return response,
    };
    let student = match student_profile(db.get_ref(), user.id).await {
        Ok(Some(student)) => student,
        Ok(None) => return HttpResponse::Forbidden().body("No student profile found."),
        Err(error) => return HttpResponse::InternalServerError().body(error.to_string()),
    };

    let code = form.code.trim().to_uppercase();
    if code.is_empty() {
        set_flash(&session, FLASH_ERROR, "Enter the attendance code.");
        return redirect("/student/attendance");
    }

    let session_row = match sqlx::query_as::<_, SessionCheck>(
        "SELECT s.id, s.status, s.opened_at, s.late_after_minutes
         FROM attendance_sessions s
         JOIN enrollments e ON e.course_id = s.course_id
         WHERE UPPER(s.check_in_code) = $1 AND e.student_id = $2
         ORDER BY s.opened_at DESC
         LIMIT 1",
    )
    .bind(&code)
    .bind(student.id)
    .fetch_optional(db.get_ref())
    .await
    {
        Ok(Some(row)) => row,
        Ok(None) => {
            set_flash(
                &session,
                FLASH_ERROR,
                "Invalid code, or you are not enrolled in that course.",
            );
            return redirect("/student/attendance");
        }
        Err(error) => return HttpResponse::InternalServerError().body(error.to_string()),
    };

    if session_row.status != "open" {
        set_flash(&session, FLASH_ERROR, "This attendance session is closed.");
        return redirect("/student/attendance");
    }

    let existing_status = match sqlx::query_scalar::<_, String>(
        "SELECT status
         FROM attendance_records
         WHERE session_id = $1 AND student_id = $2 AND checked_in_at IS NOT NULL
         LIMIT 1",
    )
    .bind(session_row.id)
    .bind(student.id)
    .fetch_optional(db.get_ref())
    .await
    {
        Ok(status) => status,
        Err(error) => return HttpResponse::InternalServerError().body(error.to_string()),
    };

    if let Some(status) = existing_status {
        set_flash(
            &session,
            FLASH_SUCCESS,
            &format!("You already checked in as {}.", status_label(&status)),
        );
        return redirect("/student/attendance");
    }

    let cutoff = session_row.opened_at + Duration::minutes(session_row.late_after_minutes as i64);
    let new_status = if Utc::now() <= cutoff {
        "present"
    } else {
        "late"
    };

    let saved_status = match sqlx::query_scalar::<_, String>(
        "INSERT INTO attendance_records (session_id, student_id, status, checked_in_at, updated_at)
         VALUES ($1, $2, $3, NOW(), NOW())
         ON CONFLICT (session_id, student_id)
         DO UPDATE SET status = EXCLUDED.status,
                       checked_in_at = NOW(),
                       marked_by = NULL,
                       note = NULL,
                       updated_at = NOW()
         RETURNING status",
    )
    .bind(session_row.id)
    .bind(student.id)
    .bind(new_status)
    .fetch_one(db.get_ref())
    .await
    {
        Ok(status) => status,
        Err(error) => return HttpResponse::InternalServerError().body(error.to_string()),
    };

    set_flash(
        &session,
        FLASH_SUCCESS,
        &format!("Checked in as {}.", status_label(&saved_status)),
    );
    redirect("/student/attendance")
}

async fn lecturer_profile(
    db: &PgPool,
    user_id: i32,
) -> Result<Option<LecturerProfile>, sqlx::Error> {
    sqlx::query_as::<_, LecturerProfile>("SELECT id FROM lecturers WHERE user_id = $1")
        .bind(user_id)
        .fetch_optional(db)
        .await
}

async fn student_profile(db: &PgPool, user_id: i32) -> Result<Option<StudentProfile>, sqlx::Error> {
    sqlx::query_as::<_, StudentProfile>("SELECT id FROM students WHERE user_id = $1")
        .bind(user_id)
        .fetch_optional(db)
        .await
}

async fn lecturer_courses(db: &PgPool, lecturer_id: i32) -> Result<Vec<CourseOption>, sqlx::Error> {
    sqlx::query_as::<_, CourseOption>(
        "SELECT id, course_code AS code, course_name AS name
         FROM courses
         WHERE lecturer_id = $1
         ORDER BY course_code",
    )
    .bind(lecturer_id)
    .fetch_all(db)
    .await
}

async fn student_courses(db: &PgPool, student_id: i32) -> Result<Vec<CourseOption>, sqlx::Error> {
    sqlx::query_as::<_, CourseOption>(
        "SELECT c.id, c.course_code AS code, c.course_name AS name
         FROM enrollments e
         JOIN courses c ON c.id = e.course_id
         WHERE e.student_id = $1
         ORDER BY c.course_code",
    )
    .bind(student_id)
    .fetch_all(db)
    .await
}

async fn lecturer_owns_course(
    db: &PgPool,
    lecturer_id: i32,
    course_id: i32,
) -> Result<bool, sqlx::Error> {
    let found =
        sqlx::query_scalar::<_, i32>("SELECT id FROM courses WHERE id = $1 AND lecturer_id = $2")
            .bind(course_id)
            .bind(lecturer_id)
            .fetch_optional(db)
            .await?;
    Ok(found.is_some())
}

async fn lecturer_can_manage_session(
    db: &PgPool,
    user_id: i32,
    session_id: i32,
) -> Result<bool, sqlx::Error> {
    let found = sqlx::query_scalar::<_, i32>(
        "SELECT s.id
         FROM attendance_sessions s
         JOIN courses c ON c.id = s.course_id
         JOIN lecturers l ON l.id = c.lecturer_id
         WHERE s.id = $1 AND l.user_id = $2",
    )
    .bind(session_id)
    .bind(user_id)
    .fetch_optional(db)
    .await?;
    Ok(found.is_some())
}

async fn manageable_record_session_id(
    db: &PgPool,
    user_id: i32,
    record_id: i32,
) -> Result<Option<i32>, sqlx::Error> {
    let found = sqlx::query_scalar::<_, i32>(
        "SELECT s.id
         FROM attendance_records ar
         JOIN attendance_sessions s ON s.id = ar.session_id
         JOIN courses c ON c.id = s.course_id
         JOIN lecturers l ON l.id = c.lecturer_id
         WHERE ar.id = $1 AND l.user_id = $2",
    )
    .bind(record_id)
    .bind(user_id)
    .fetch_optional(db)
    .await?;
    Ok(found)
}

async fn lecturer_course_overviews(
    db: &PgPool,
    lecturer_id: i32,
) -> Result<Vec<LecturerCourseOverview>, sqlx::Error> {
    sqlx::query_as::<_, LecturerCourseOverview>(
        "SELECT
             c.id AS course_id,
             c.course_code,
             c.course_name,
             COUNT(DISTINCT e.student_id) AS enrolled_count,
             COUNT(DISTINCT s.id) AS total_sessions,
             COUNT(ar.id) AS total_records,
             COALESCE(
                 ROUND(
                     COUNT(ar.id) FILTER (WHERE ar.status IN ('present', 'late', 'excused'))::NUMERIC
                     * 100 / NULLIF(COUNT(ar.id), 0)
                 )::INT,
                 0
             ) AS attendance_pct,
             COUNT(ar.id) FILTER (WHERE ar.status = 'present') AS present_count,
             COUNT(ar.id) FILTER (WHERE ar.status = 'late') AS late_count,
             COUNT(ar.id) FILTER (WHERE ar.status = 'absent') AS absent_count,
             COUNT(ar.id) FILTER (WHERE ar.status = 'excused') AS excused_count,
             latest.session_title AS latest_session_title,
             latest.status AS latest_session_status,
             CASE
                 WHEN latest.opened_at IS NULL THEN NULL
                ELSE TO_CHAR(latest.opened_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"')
             END AS latest_session_opened_at
         FROM courses c
         LEFT JOIN enrollments e ON e.course_id = c.id
         LEFT JOIN attendance_sessions s ON s.course_id = c.id
         LEFT JOIN attendance_records ar ON ar.session_id = s.id
         LEFT JOIN LATERAL (
             SELECT session_title, status, opened_at
             FROM attendance_sessions
             WHERE course_id = c.id
             ORDER BY opened_at DESC
             LIMIT 1
         ) latest ON TRUE
         WHERE c.lecturer_id = $1
         GROUP BY c.id, c.course_code, c.course_name, latest.session_title, latest.status, latest.opened_at
         ORDER BY c.course_code",
    )
    .bind(lecturer_id)
    .fetch_all(db)
    .await
}

async fn lecturer_sessions(
    db: &PgPool,
    lecturer_id: i32,
) -> Result<Vec<LecturerSessionView>, sqlx::Error> {
    let rows = sqlx::query_as::<_, LecturerSessionRow>(
        "WITH recent_sessions AS (
             SELECT
                 s.id,
                 s.course_id,
                 s.session_title,
                 s.check_in_code,
                 s.status,
                 s.late_after_minutes,
                 s.opened_at,
                 s.closed_at
             FROM attendance_sessions s
             JOIN courses c ON c.id = s.course_id
             WHERE c.lecturer_id = $1
             ORDER BY (s.status = 'open') DESC, s.opened_at DESC
             LIMIT 20
         )
         SELECT
             s.id,
             s.course_id,
             c.course_code,
             c.course_name,
             s.session_title,
             s.check_in_code,
             s.status,
             s.late_after_minutes,
             TO_CHAR(s.opened_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS opened_at,
             CASE
                 WHEN s.closed_at IS NULL THEN NULL
                 ELSE TO_CHAR(s.closed_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"')
             END AS closed_at,
             COUNT(ar.id) AS total_count,
             COUNT(ar.id) FILTER (WHERE ar.status = 'present') AS present_count,
             COUNT(ar.id) FILTER (WHERE ar.status = 'late') AS late_count,
             COUNT(ar.id) FILTER (WHERE ar.status = 'absent') AS absent_count,
             COUNT(ar.id) FILTER (WHERE ar.status = 'excused') AS excused_count
         FROM recent_sessions s
         JOIN courses c ON c.id = s.course_id
         LEFT JOIN attendance_records ar ON ar.session_id = s.id
         GROUP BY s.id, s.course_id, c.course_code, c.course_name, s.session_title,
                  s.check_in_code, s.status, s.late_after_minutes, s.opened_at, s.closed_at
         ORDER BY (s.status = 'open') DESC, s.opened_at DESC",
    )
    .bind(lecturer_id)
    .fetch_all(db)
    .await?;

    let mut sessions = Vec::with_capacity(rows.len());
    for row in rows {
        sessions.push(session_view_from_row(row, Vec::new()));
    }

    Ok(sessions)
}

async fn lecturer_session(
    db: &PgPool,
    lecturer_id: i32,
    session_id: i32,
) -> Result<Option<LecturerSessionView>, sqlx::Error> {
    let row = sqlx::query_as::<_, LecturerSessionRow>(
        "SELECT
             s.id,
             s.course_id,
             c.course_code,
             c.course_name,
             s.session_title,
             s.check_in_code,
             s.status,
             s.late_after_minutes,
             TO_CHAR(s.opened_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS opened_at,
             CASE
                 WHEN s.closed_at IS NULL THEN NULL
                 ELSE TO_CHAR(s.closed_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"')
             END AS closed_at,
             COUNT(ar.id) AS total_count,
             COUNT(ar.id) FILTER (WHERE ar.status = 'present') AS present_count,
             COUNT(ar.id) FILTER (WHERE ar.status = 'late') AS late_count,
             COUNT(ar.id) FILTER (WHERE ar.status = 'absent') AS absent_count,
             COUNT(ar.id) FILTER (WHERE ar.status = 'excused') AS excused_count
         FROM attendance_sessions s
         JOIN courses c ON c.id = s.course_id
         LEFT JOIN attendance_records ar ON ar.session_id = s.id
         WHERE c.lecturer_id = $1 AND s.id = $2
         GROUP BY s.id, c.course_code, c.course_name",
    )
    .bind(lecturer_id)
    .bind(session_id)
    .fetch_optional(db)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    let records = lecturer_records(db, row.id).await?;
    Ok(Some(session_view_from_row(row, records)))
}

async fn lecturer_records(
    db: &PgPool,
    session_id: i32,
) -> Result<Vec<LecturerRecordView>, sqlx::Error> {
    sqlx::query_as::<_, LecturerRecordView>(
        "SELECT
             latest.id,
             latest.student_name,
             latest.student_email,
             latest.status,
             latest.checked_in_at,
             latest.note
         FROM (
             SELECT DISTINCT ON (st.id)
                 ar.id,
                 st.id AS student_id,
                 u.display_name AS student_name,
                 u.email AS student_email,
                 ar.status,
                 CASE
                     WHEN ar.checked_in_at IS NULL THEN NULL
                     ELSE TO_CHAR(ar.checked_in_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"')
                 END AS checked_in_at,
                 COALESCE(ar.note, '') AS note,
                 ar.updated_at
             FROM attendance_records ar
             JOIN students st ON st.id = ar.student_id
             JOIN users u ON u.id = st.user_id
             WHERE ar.session_id = $1
             ORDER BY st.id, ar.updated_at DESC, ar.id DESC
         ) latest
         ORDER BY latest.student_name",
    )
    .bind(session_id)
    .fetch_all(db)
    .await
}

fn session_view_from_row(
    row: LecturerSessionRow,
    records: Vec<LecturerRecordView>,
) -> LecturerSessionView {
    LecturerSessionView {
        id: row.id,
        course_id: row.course_id,
        course_code: row.course_code,
        course_name: row.course_name,
        session_title: row.session_title,
        check_in_code: row.check_in_code,
        status: row.status,
        late_after_minutes: row.late_after_minutes,
        opened_at: row.opened_at,
        closed_at: row.closed_at,
        total_count: row.total_count,
        present_count: row.present_count,
        late_count: row.late_count,
        absent_count: row.absent_count,
        excused_count: row.excused_count,
        records,
    }
}

async fn student_attendance_courses(
    db: &PgPool,
    student_id: i32,
    courses: &[CourseOption],
) -> Result<Vec<StudentAttendanceCourseView>, sqlx::Error> {
    let mut result = Vec::with_capacity(courses.len());

    for course in courses {
        let sessions = sqlx::query_as::<_, StudentAttendanceSessionView>(
            "SELECT
                 TO_CHAR(s.opened_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS date,
                 s.session_title AS topic,
                 COALESCE(ar.status, 'absent') AS status
             FROM attendance_sessions s
             LEFT JOIN attendance_records ar
                 ON ar.session_id = s.id AND ar.student_id = $1
             WHERE s.course_id = $2
             ORDER BY s.opened_at DESC",
        )
        .bind(student_id)
        .bind(course.id)
        .fetch_all(db)
        .await?;

        let total = sessions.len() as i32;
        let attended = sessions
            .iter()
            .filter(|record| matches!(record.status.as_str(), "present" | "late" | "excused"))
            .count() as i32;
        let pct = if total > 0 {
            (attended * 100) / total
        } else {
            0
        };

        result.push(StudentAttendanceCourseView {
            id: course.id,
            code: course.code.clone(),
            name: course.name.clone(),
            pct,
            attended,
            total,
            sessions,
        });
    }

    Ok(result)
}

async fn student_active_sessions(
    db: &PgPool,
    student_id: i32,
) -> Result<Vec<StudentActiveSessionView>, sqlx::Error> {
    sqlx::query_as::<_, StudentActiveSessionView>(
        "SELECT
             s.id,
             c.course_code,
             c.course_name,
             s.session_title,
             s.late_after_minutes,
             TO_CHAR(s.opened_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS opened_at,
             ar.status AS checked_in_status,
             CASE
                 WHEN ar.checked_in_at IS NULL THEN NULL
                 ELSE TO_CHAR(ar.checked_in_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"')
             END AS checked_in_at
         FROM attendance_sessions s
         JOIN courses c ON c.id = s.course_id
         JOIN enrollments e ON e.course_id = c.id
         LEFT JOIN attendance_records ar
             ON ar.session_id = s.id AND ar.student_id = e.student_id
         WHERE e.student_id = $1 AND s.status = 'open'
         ORDER BY s.opened_at DESC",
    )
    .bind(student_id)
    .fetch_all(db)
    .await
}

fn insert_lecturer_base(ctx: &mut Context, display_name: &str) {
    ctx.insert("display_name", display_name);
    ctx.insert("student_name", display_name);
    ctx.insert("student_id", "");
    ctx.insert("notifications", &Vec::<crate::NotificationContext>::new());
    ctx.insert("active_page", "attendance");
    ctx.insert("is_lecturer", &true);
}

fn take_flash(session: &Session, ctx: &mut Context) {
    if let Ok(Some(message)) = session.get::<String>(FLASH_SUCCESS) {
        ctx.insert(FLASH_SUCCESS, &message);
        session.remove(FLASH_SUCCESS);
    }
    if let Ok(Some(message)) = session.get::<String>(FLASH_ERROR) {
        ctx.insert(FLASH_ERROR, &message);
        session.remove(FLASH_ERROR);
    }
}

fn set_flash(session: &Session, key: &str, message: &str) {
    let _ = session.insert(key, message);
}

fn redirect(path: &str) -> HttpResponse {
    HttpResponse::SeeOther()
        .insert_header((header::LOCATION, path))
        .finish()
}

fn render(tmpl: &Tera, template: &str, ctx: &Context) -> HttpResponse {
    match tmpl.render(template, ctx) {
        Ok(html) => HttpResponse::Ok().content_type("text/html").body(html),
        Err(error) => HttpResponse::InternalServerError()
            .body(format!("Failed to render '{template}': {error}")),
    }
}

fn generate_check_in_code() -> String {
    const CHARS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut rng = OsRng;
    (0..6)
        .map(|_| {
            let idx = (rng.next_u32() as usize) % CHARS.len();
            CHARS[idx] as char
        })
        .collect()
}

fn status_label(status: &str) -> &'static str {
    match status {
        "present" => "present",
        "late" => "late",
        "excused" => "excused",
        _ => "absent",
    }
}
