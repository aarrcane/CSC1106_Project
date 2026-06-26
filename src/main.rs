use actix_files::Files;
use actix_session::{Session, SessionMiddleware, storage::CookieSessionStore};
use actix_web::{
    App, HttpResponse, HttpServer, Responder,
    cookie::Key,
    middleware::{NormalizePath, TrailingSlash},
    web,
};
use tera::{Context, Tera};

use argon2::password_hash::{SaltString, rand_core::OsRng};
use argon2::{Argon2, PasswordHasher};
// use dotenvy::dotenv;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool, postgres::PgPoolOptions};
use std::env;

mod admin;
mod attendance;
mod auth;
mod forum;
mod lecturer;
mod storage;
mod student;
mod student_quiz;
mod quiz_engine;
use storage::SupabaseStorage;
use crate::admin::{log_audit_event, AuditActor};

// ─── Shared Context Types ─────────────────────────────────────────────────────
mod lecturer_quiz;

#[derive(Serialize)]
struct CourseContext {
    id: i32,
    code: String,
    name: String,
    trimester: String,
    image_url: String,
    pinned: bool,
    ongoing: bool,
    progress: i32,
    lecturer: String,
    attendance_pct: i32,
}

#[derive(Serialize)]
struct AnnouncementContext {
    title: String,
    course: String,
    date: String,
}

#[derive(Serialize)]
struct AnnouncementFullContext {
    id: i32,
    title: String,
    course: String,
    course_code: String,
    date: String,
    content: String,
    is_new: bool,
}

#[derive(Serialize)]
struct DueDateContext {
    title: String,
    course: String,
    #[serde(rename = "type")]
    item_type: String,
    due_date: String,
    urgent: bool,
}

#[derive(Serialize)]
struct NotificationContext {
    icon_class: String,
    text: String,
    sub_text: String,
}

#[derive(Serialize)]
struct AssignmentContext {
    id: i32,
    title: String,
    course_code: String,
    course_name: String,
    item_type: String,
    due_date: String,
    status: String,
    score: Option<String>,
    urgent: bool,
}

#[derive(Serialize)]
struct GradeItemContext {
    title: String,
    item_type: String,
    score: f32,
    max_score: f32,
    weight: f32,
}

#[derive(Serialize)]
struct CourseGradeContext {
    code: String,
    name: String,
    overall: f32,
    grade_letter: String,
    items: Vec<GradeItemContext>,
}

#[derive(Serialize)]
struct QuizContext {
    id: i32,
    title: String,
    course_code: String,
    course_name: String,
    due_date: String,
    duration_mins: i32,
    status: String,
    score: Option<String>,
    total_marks: i32,
    attempt_allowed: i32,
    attempts_used: i32,
    urgent: bool,
}

#[derive(Serialize)]
struct QuizAttemptQuestionContext {
    number: i32,
    prompt: String,
    options: Vec<String>,
}

#[derive(Deserialize)]
struct QuizMonitoringEventPayload {
    event_type: String,
    severity: String,
    details: Option<String>,
}

#[derive(Serialize)]
struct QuizMonitoringEventResponse {
    status: &'static str,
}

#[derive(Serialize, FromRow)]
struct QuizMonitoringEventContext {
    id: i32,
    quiz_id: i32,
    student_display_name: String,
    event_type: String,
    severity: String,
    details: Option<String>,
    occurred_at: String,
}

#[derive(Serialize)]
struct AttendanceSessionContext {
    date: String,
    topic: String,
    status: String,
}

#[derive(Serialize)]
struct AttendanceCourseContext {
    code: String,
    name: String,
    pct: i32,
    attended: i32,
    total: i32,
    sessions: Vec<AttendanceSessionContext>,
}

#[derive(Serialize)]
struct ThreadContext {
    id: i32,
    title: String,
    course_code: String,
    course_name: String,
    author: String,
    author_initials: String,
    created_at: String,
    last_reply_at: String,
    reply_count: i32,
    view_count: i32,
    is_pinned: bool,
    is_answered: bool,
    is_mine: bool,
    tags: Vec<String>,
    preview: String,
}

// ─── Helper Functions ─────────────────────────────────────────────────────────

fn insert_student_base(ctx: &mut Context, display_name: &str, student_id: &str) {
    ctx.insert("student_name", display_name);
    ctx.insert("student_id", student_id);
    let notifications: Vec<NotificationContext> = vec![];
    ctx.insert("notifications", &notifications);
}

fn mock_quiz_attempt(quiz_id: i32) -> Option<QuizContext> {
    match quiz_id {
        2 => Some(QuizContext {
            id: 2,
            title: "Quiz 2 - JavaScript Fundamentals".into(),
            course_code: "CSC1106".into(),
            course_name: "Web Programming".into(),
            due_date: "28 May 2026".into(),
            duration_mins: 25,
            status: "open".into(),
            score: None,
            total_marks: 25,
            attempt_allowed: 2,
            attempts_used: 0,
            urgent: true,
        }),
        _ => None,
    }
}

fn mock_quiz_questions() -> Vec<QuizAttemptQuestionContext> {
    vec![
        QuizAttemptQuestionContext {
            number: 1,
            prompt: "Which keyword declares a block-scoped JavaScript variable?".into(),
            options: vec!["var".into(), "let".into(), "static".into(), "global".into()],
        },
        QuizAttemptQuestionContext {
            number: 2,
            prompt: "Which browser API is commonly used to request JSON data asynchronously?"
                .into(),
            options: vec![
                "Fetch API".into(),
                "Canvas API".into(),
                "Storage API".into(),
                "History API".into(),
            ],
        },
        QuizAttemptQuestionContext {
            number: 3,
            prompt: "What does DOM stand for?".into(),
            options: vec![
                "Document Object Model".into(),
                "Data Object Map".into(),
                "Display Output Method".into(),
                "Document Order Mode".into(),
            ],
        },
    ]
}

fn valid_monitoring_event_type(event_type: &str) -> bool {
    matches!(
        event_type,
        "monitoring_started"
            | "monitoring_error"
            | "camera_permission_denied"
            | "microphone_permission_denied"
            | "face_missing"
            | "face_restored"
            | "multiple_faces"
            | "looking_away"
            | "noise_spike"
    )
}

fn valid_monitoring_severity(severity: &str) -> bool {
    matches!(severity, "info" | "warning" | "critical")
}

fn truncate_details(details: Option<&str>) -> Option<String> {
    let details = details?.trim();
    if details.is_empty() {
        return None;
    }
    Some(details.chars().take(500).collect())
}

fn quiz_monitoring_ready_key(quiz_id: i32) -> String {
    format!("quiz_{quiz_id}_monitoring_ready")
}

fn hash_password(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| format!("Failed to hash password: {error}"))
}

fn generate_temp_password(len: usize) -> String {
    let salt = SaltString::generate(&mut OsRng);
    salt.as_str().chars().take(len).collect()
}

fn parse_optional_i32(value: Option<&str>, field_name: &str) -> Result<Option<i32>, String> {
    let Some(raw) = value else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    trimmed
        .parse::<i32>()
        .map(Some)
        .map_err(|_| format!("{field_name} must be a whole number."))
}

fn quiz_monitoring_ready(session: &Session, quiz_id: i32) -> Result<bool, HttpResponse> {
    session
        .get::<bool>(&quiz_monitoring_ready_key(quiz_id))
        .map(|ready| ready.unwrap_or(false))
        .map_err(|error| {
            HttpResponse::InternalServerError()
                .body(format!("Failed to read quiz monitoring session: {error}"))
        })
}

fn session_key() -> Key {
    let secret =
        env::var("SESSION_SECRET").unwrap_or_else(|_| "01234567890123456789012345678901".into());
    Key::from(secret.as_bytes())
}

// ─── Password Change ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct PasswordChangeForm {
    new_password: String,
    confirm_password: String,
}

async fn password_change_page(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let display_name = session
        .get::<String>("display_name")
        .ok()
        .flatten()
        .unwrap_or_else(|| "User".to_string());
    let role = session
        .get::<String>("role")
        .ok()
        .flatten()
        .unwrap_or_default();

    let mut ctx = Context::new();
    ctx.insert("student_name", &display_name);
    ctx.insert("student_id", "");
    ctx.insert("notifications", &Vec::<NotificationContext>::new());
    ctx.insert("is_admin", &(role == "admin"));
    ctx.insert("is_lecturer", &(role == "lecturer"));

    let rendered = tmpl
        .render("password_change.html", &ctx)
        .unwrap_or_else(|e| e.to_string());
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

async fn password_change_submit(
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
    form: web::Form<PasswordChangeForm>,
) -> impl Responder {
    let display_name = session
        .get::<String>("display_name")
        .ok()
        .flatten()
        .unwrap_or_else(|| "User".to_string());
    let role = session
        .get::<String>("role")
        .ok()
        .flatten()
        .unwrap_or_default();

    if form.new_password.len() < 8 {
        let mut ctx = Context::new();
        ctx.insert("student_name", &display_name);
        ctx.insert("student_id", "");
        ctx.insert("notifications", &Vec::<NotificationContext>::new());
        ctx.insert("is_admin", &(role == "admin"));
        ctx.insert("is_lecturer", &(role == "lecturer"));
        ctx.insert(
            "error_message",
            "Password must be at least 8 characters long.",
        );
        let rendered = tmpl
            .render("password_change.html", &ctx)
            .unwrap_or_else(|e| e.to_string());
        return HttpResponse::Ok().content_type("text/html").body(rendered);
    }

    if form.new_password != form.confirm_password {
        let mut ctx = Context::new();
        ctx.insert("student_name", &display_name);
        ctx.insert("student_id", "");
        ctx.insert("notifications", &Vec::<NotificationContext>::new());
        ctx.insert("is_admin", &(role == "admin"));
        ctx.insert("is_lecturer", &(role == "lecturer"));
        ctx.insert("error_message", "Passwords do not match.");
        let rendered = tmpl
            .render("password_change.html", &ctx)
            .unwrap_or_else(|e| e.to_string());
        return HttpResponse::Ok().content_type("text/html").body(rendered);
    }

    let Some(user_id) = session.get::<i32>("user_id").ok().flatten() else {
        return HttpResponse::SeeOther()
            .insert_header(("Location", "/login"))
            .finish();
    };

    let password_hash = match hash_password(&form.new_password) {
        Ok(hash) => hash,
        Err(error) => return HttpResponse::InternalServerError().body(error),
    };

    let result = sqlx::query(
        "UPDATE users SET password_hash = $1, must_change_password = FALSE WHERE id = $2",
    )
    .bind(&password_hash)
    .bind(user_id)
    .execute(db.get_ref())
    .await;

    if let Err(error) = result {
        return HttpResponse::InternalServerError()
            .body(format!("Failed to update password: {error}"));
    }

    let actor = AuditActor {
        user_id: Some(user_id),
        role: Some(role.clone()),
        display_name: Some(display_name.clone()),
    };
    log_audit_event(
        db.get_ref(),
        "auth",
        "password_changed",
        "warning",
        &actor,
        Some("account"),
        Some(user_id),
        Some("Updated account password".to_string()),
    )
    .await;

    let Some(role) = auth::UserRole::from_slug(&role) else {
        return HttpResponse::SeeOther()
            .insert_header(("Location", "/"))
            .finish();
    };

    HttpResponse::SeeOther()
        .insert_header(("Location", role.home_path()))
        .finish()
}

// ─── Index ────────────────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct LoginPageQuery {
    logged_out: Option<String>,
}

async fn index(
    tmpl: web::Data<Tera>,
    session: Session,
    query: web::Query<LoginPageQuery>,
) -> impl Responder {
    match auth::redirect_authenticated_user(&session) {
        Ok(Some(response)) => return response,
        Ok(None) => {}
        Err(response) => return response,
    }

    let mut ctx = Context::new();
    ctx.insert("email_value", "");
    ctx.insert("error_message", "");
    ctx.insert("has_error", &false);
    if query.logged_out.is_some() {
        ctx.insert("logout_message", "You have been signed out.");
    }
    let rendered = tmpl.render("index.html", &ctx).unwrap();

    HttpResponse::Ok().content_type("text/html").body(rendered)
}

// ─── Main ─────────────────────────────────────────────────────────────────────

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenvy::dotenv().ok();

    let tera = Tera::new("templates/**/*").unwrap();
    let storage = SupabaseStorage::from_env();

    let database_url = match env::var("DATABASE_URL") {
        Ok(val) => val,
        Err(_) => {
            eprintln!("ERROR: DATABASE_URL not set.");
            std::process::exit(1);
        }
    };

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to connect to PostgreSQL");

    let session_key = session_key();
    println!("Connected to server");

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(storage.clone()))
            .wrap(NormalizePath::new(TrailingSlash::Trim))
            .wrap(
                SessionMiddleware::builder(CookieSessionStore::default(), session_key.clone())
                    .cookie_secure(false)
                    .build(),
            )
            .app_data(web::Data::new(tera.clone()))
            .app_data(web::Data::new(pool.clone()))
            // ── Static ────────────────────────────────────────────────────────
            .service(Files::new("/static", "./static"))
            // ── Public ───────────────────────────────────────────────────────
            .route("/", web::get().to(index))
            .route("/login", web::get().to(index))
            .route("/login", web::post().to(auth::login_submit))
            .route("/logout", web::post().to(auth::logout))
            .route("/password/change", web::get().to(password_change_page))
            .route("/password/change", web::post().to(password_change_submit))
            // ── Student ───────────────────────────────────────────────────────
            .route(
                "/student/dashboard",
                web::get().to(student::student_dashboard),
            )
            .route("/student/courses", web::get().to(student::student_courses))
            .route(
                "/student/assignments",
                web::get().to(student::student_assignments),
            )
            .route("/student/grades", web::get().to(student::student_grades))
            .route(
                "/student/announcement",
                web::get().to(student::student_announcement),
            )
            .route(
                "/student/attendance",
                web::get().to(attendance::student_attendance),
            )
            .route(
                "/student/attendance/check-in",
                web::post().to(attendance::student_check_in),
            )
            .route("/student/forum", web::get().to(forum::student_forum))
            .route(
                "/student/forum/new",
                web::post().to(forum::create_student_thread),
            )
            .route(
                "/student/courses/{course_id}/forum",
                web::get().to(forum::student_course_forum),
            )
            .route(
                "/student/forum/threads/{thread_id}",
                web::get().to(forum::student_thread_detail),
            )
            .route(
                "/student/forum/threads/{thread_id}/reply",
                web::post().to(forum::add_student_reply),
            )
            .route(
                "/student/forum/threads/{thread_id}/edit",
                web::post().to(forum::edit_student_thread),
            )
            .route(
                "/student/forum/threads/{thread_id}/delete",
                web::post().to(forum::delete_student_thread),
            )
            .route(
                "/student/forum/posts/{post_id}/edit",
                web::post().to(forum::edit_student_post),
            )
            .route(
                "/student/forum/posts/{post_id}/delete",
                web::post().to(forum::delete_student_post),
            )
            .route(
                "/student/forum/attachments/{attachment_id}/delete",
                web::post().to(forum::delete_student_attachment),
            )
            .route(
                "/student/course/{id}/data",
                web::get().to(student::student_course_data),
            )
            .route(
                "/student/profile",
                web::get().to(student::student_profile_page),
            )
            .route(
                "/student/settings",
                web::get().to(student::student_settings_page),
            )
            .route(
                "/student/settings",
                web::post().to(student::student_settings_submit),
            )
            // ── Student quizzes ───────────────────────
            .route("/student/quizzes", web::get().to(student_quiz::quiz_list))
            .route(
                "/student/quizzes/{quiz_id}/attempt",
                web::get().to(student_quiz::attempt_gate),
            )
            .route(
                "/student/quizzes/{quiz_id}/take",
                web::get().to(student_quiz::take),
            )
            .route(
                "/student/quizzes/{quiz_id}/submit",
                web::post().to(student_quiz::submit),
            )
            .route(
                "/student/quizzes/{quiz_id}/result",
                web::get().to(student_quiz::result),
            )
            .route(
                "/student/quizzes/{quiz_id}/monitoring-ready",
                web::post().to(student_quiz::mark_monitoring_ready),
            )
            .route(
                "/student/quizzes/{quiz_id}/monitoring-events",
                web::post().to(student_quiz::save_monitoring_event),
            )
            .route(
                "/student/assignments/data",
                web::get().to(student::student_assignments_data),
            )
            .route(
                "/student/assignments/submit",
                web::post().to(student::student_assignment_submit),
            )
            // ── Lecturer ──────────────────────────────────────────────────────
            .route(
                "/lecturer/dashboard",
                web::get().to(lecturer::lecturer_dashboard),
            )
            .route(
                "/lecturer/courses",
                web::get().to(lecturer::lecturer_courses_page),
            )
            .route(
                "/lecturer/course/{id}/data",
                web::get().to(lecturer::lecturer_course_data),
            )
            .route(
                "/lecturer/course/{id}/week/create",
                web::post().to(lecturer::create_week),
            )
            .route(
                "/lecturer/course/{cid}/week/{wid}/upload",
                web::post().to(lecturer::upload_material),
            )
            .route(
                "/lecturer/course/{cid}/week/{wid}/delete",
                web::delete().to(lecturer::delete_week),
            )
            .route(
                "/lecturer/material/{id}/delete",
                web::delete().to(lecturer::delete_material),
            )
            .route(
                "/lecturer/assignments",
                web::get().to(lecturer::lecturer_assignments_page),
            )
            .route(
                "/lecturer/quizzes",
                web::get().to(lecturer::lecturer_quizzes_page),
            )
            .route(
                "/lecturer/grades",
                web::get().to(lecturer::lecturer_grades_page),
            )
            .route(
                "/lecturer/attendance",
                web::get().to(attendance::lecturer_attendance),
            )
            .route(
                "/lecturer/attendance/sessions",
                web::get().to(attendance::lecturer_attendance_sessions),
            )
            .route(
                "/lecturer/attendance/sessions",
                web::post().to(attendance::create_session),
            )
            .route(
                "/lecturer/attendance/sessions/{session_id}",
                web::get().to(attendance::lecturer_attendance_session_detail),
            )
            .route(
                "/lecturer/attendance/sessions/{session_id}/close",
                web::post().to(attendance::close_session),
            )
            .route(
                "/lecturer/attendance/sessions/{session_id}/delete",
                web::post().to(attendance::delete_session),
            )
            .route(
                "/lecturer/attendance/records/{record_id}",
                web::post().to(attendance::update_record),
            )
            .route("/lecturer/forum", web::get().to(forum::lecturer_forum))
            .route(
                "/lecturer/forum/new",
                web::post().to(forum::create_lecturer_thread),
            )
            .route(
                "/lecturer/courses/{course_id}/forum",
                web::get().to(forum::lecturer_course_forum),
            )
            .route(
                "/lecturer/forum/threads/{thread_id}",
                web::get().to(forum::lecturer_thread_detail),
            )
            .route(
                "/lecturer/forum/threads/{thread_id}/reply",
                web::post().to(forum::add_lecturer_reply),
            )
            .route(
                "/lecturer/forum/threads/{thread_id}/{action}",
                web::post().to(forum::moderate_thread),
            )
            .route(
                "/lecturer/forum/posts/{post_id}/edit",
                web::post().to(forum::edit_lecturer_post),
            )
            .route(
                "/lecturer/forum/posts/{post_id}/{action}",
                web::post().to(forum::moderate_post),
            )
            .route(
                "/lecturer/forum/attachments/{attachment_id}/{action}",
                web::post().to(forum::moderate_attachment),
            )
            .route(
                "/lecturer/profile",
                web::get().to(lecturer::lecturer_profile_page),
            )
            .route(
                "/lecturer/settings",
                web::get().to(lecturer::lecturer_settings_page),
            )
            .route(
                "/lecturer/settings",
                web::post().to(lecturer::lecturer_settings_submit),
            )
            .route(
                "/lecturer/assignments/data",
                web::get().to(lecturer::lecturer_assignments_data),
            )
            .route(
                "/lecturer/assignments/create",
                web::post().to(lecturer::create_assignment),
            )
            .route(
                "/lecturer/assignments/{id}/delete",
                web::delete().to(lecturer::delete_assignment),
            )
            .route(
                "/lecturer/submissions/{id}/grade",
                web::post().to(lecturer::grade_submission),
            )
            .configure(lecturer_quiz::config)
            // Admin Routes
            .route("/admin/dashboard", web::get().to(admin::admin_dashboard))
            .route("/admin/profile", web::get().to(admin::admin_profile_page))
            .route("/admin/users", web::get().to(admin::admin_users_page))
            .route(
                "/admin/users/create",
                web::post().to(admin::admin_create_user),
            )
            .route(
                "/admin/users/{id}/toggle-active",
                web::post().to(admin::admin_toggle_user_active),
            )
            .route(
                "/admin/users/{id}/update",
                web::post().to(admin::admin_update_user),
            )
            .route(
                "/admin/users/{id}/reset-password",
                web::post().to(admin::admin_reset_user_password),
            )
            .route("/admin/courses", web::get().to(admin::admin_courses_page))
            .route("/admin/settings", web::get().to(admin::admin_settings_page))
            .route("/admin/settings", web::post().to(admin::admin_settings_submit))
            .route("/admin/audit", web::get().to(admin::admin_audit_page))
            .route("/admin/course/create", web::post().to(admin::create_course))
            .route(
                "/admin/course/{id}/assign",
                web::post().to(admin::assign_lecturer),
            )
            .route(
                "/admin/course/{id}/delete",
                web::delete().to(admin::delete_course),
            )
            .route("/admin/content", web::get().to(admin::admin_content_page))
            .route(
                "/admin/content/forum/thread/{id}/{action}",
                web::post().to(admin::admin_moderate_forum_thread),
            )
            .route(
                "/admin/content/forum/post/{id}/{action}",
                web::post().to(admin::admin_moderate_forum_post),
            )
            .route(
                "/admin/course/{id}/enrollments",
                web::get().to(admin::get_course_enrollments),
            )
            .route(
                "/admin/course/{id}/enroll",
                web::post().to(admin::enroll_student),
            )
            .route(
                "/admin/course/{id}/unenroll",
                web::post().to(admin::unenroll_student),
            )
            .configure(quiz_engine::config)
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
