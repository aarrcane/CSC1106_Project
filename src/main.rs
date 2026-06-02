use actix_files::Files;
use actix_session::{Session, SessionMiddleware, storage::CookieSessionStore};
use actix_web::{
    cookie::Key,
    middleware::{NormalizePath, TrailingSlash},
    web, App, HttpResponse, HttpServer, Responder,
};
use tera::{Context, Tera};

use dotenvy::dotenv;
use argon2::{Argon2, PasswordHasher};
use argon2::password_hash::{SaltString, rand_core::OsRng};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool, postgres::PgPoolOptions};
use std::env;

mod auth;

mod admin;
mod student;
mod lecturer;

#[derive(Serialize)]
struct CourseContext {
    id: i32,
    code: String,
    name: String,
    trimester: String,
    image_url: String,
    pinned: bool,
    ongoing: bool,
    progress: i32,     //0-100%
    lecturer: String,
    attendance_pct: i32,   //0-100%
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
    item_type: String, // "quiz" or "assignment"
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
    item_type: String, //"assignment" or "quiz"
    due_date: String,
    status: String, //"pending" | "submitted" | "late" | "graded"
    score: Option<String>,
    urgent: bool,
}

#[derive(Serialize)]
struct GradeItemContext {
    title: String,
    item_type: String, //"assignment" or "quiz"
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
    status: String,         // "upcoming" | "open" | "completed" | "missed"
    score: Option<String>, // e.g "18/25"
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
    status: String, // "present" | "absent" | "late" | "excused"
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

fn insert_student_base(ctx: &mut Context, display_name: &str, student_id: &str) {
    ctx.insert("student_name", display_name);
    ctx.insert("student_id", student_id);
    //TODO: Replace with real DB query for unread notifications
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
            prompt: "Which browser API is commonly used to request JSON data asynchronously?".into(),
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
    let secret = env::var("SESSION_SECRET").unwrap_or_else(|_| "01234567890123456789012345678901".into());
    Key::from(secret.as_bytes())
}

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
        ctx.insert("error_message", "Password must be at least 8 characters long.");

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
        Err(error) => {
            return HttpResponse::InternalServerError().body(error);
        }
    };

    let result = sqlx::query(
        r#"UPDATE users
           SET password_hash = $1,
               must_change_password = FALSE
         WHERE id = $2"#,
    )
    .bind(&password_hash)
    .bind(user_id)
    .execute(db.get_ref())
    .await;

    if let Err(error) = result {
        return HttpResponse::InternalServerError()
            .body(format!("Failed to update password: {error}"));
    }

    let Some(role) = auth::UserRole::from_slug(&role) else {
        return HttpResponse::SeeOther()
            .insert_header(("Location", "/"))
            .finish();
    };

    HttpResponse::SeeOther()
        .insert_header(("Location", role.home_path()))
        .finish()
}

async fn index(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    match auth::redirect_authenticated_user(&session) {
        Ok(Some(response)) => return response,
        Ok(None) => {}
        Err(response) => return response,
    }

    let mut ctx = Context::new();
    ctx.insert("email_value", "");
    ctx.insert("error_message", "");
    ctx.insert("has_error", &false);
    let rendered = tmpl.render("index.html", &ctx).unwrap();

    HttpResponse::Ok().content_type("text/html").body(rendered)
}

    #[actix_web::main]
    async fn main() -> std::io::Result<()> {

    // Load environment variables from .env (if present)
    dotenv().ok();

    let tera = Tera::new("templates/**/*").unwrap();

    let database_url = match env::var("DATABASE_URL") {
        Ok(val) => val,
        Err(_) => {
            eprintln!(
                "ERROR: DATABASE_URL not set. Create a .env with DATABASE_URL or set the environment variable."
            );
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
            .wrap(NormalizePath::new(TrailingSlash::Trim))
            .wrap(
                SessionMiddleware::builder(CookieSessionStore::default(), session_key.clone())
                    .cookie_secure(false)
                    .build(),
            )
            .app_data(web::Data::new(tera.clone()))
            .app_data(web::Data::new(pool.clone()))

            //Static Files (CSS, JS, images)
            .service(Files::new("/static", "./static"))

            // Public Routes
            .route("/", web::get().to(index))
            .route("/login", web::get().to(index))
            .route("/login", web::post().to(auth::login_submit))
            .route("/logout", web::post().to(auth::logout))

            // Password change (first-login)
            .route("/password/change", web::get().to(password_change_page))
            .route("/password/change", web::post().to(password_change_submit))

            // Student Routes
            .route("/student/dashboard", web::get().to(student::student_dashboard))
            .route("/student/courses", web::get().to(student::student_courses))
            .route("/student/assignments", web::get().to(student::student_assignments))
            .route("/student/grades", web::get().to(student::student_grades))
            .route("/student/announcement", web::get().to(student::student_announcement))
            .route("/student/quizzes",      web::get().to(student::student_quiz))
            .route("/student/quizzes/{quiz_id}/attempt", web::get().to(student::student_quiz_attempt))
            .route("/student/quizzes/{quiz_id}/take", web::get().to(student::student_quiz_take))
            .route("/student/quizzes/{quiz_id}/monitoring-ready", web::post().to(student::mark_quiz_monitoring_ready))
            .route("/student/quizzes/{quiz_id}/monitoring-events", web::post().to(student::save_quiz_monitoring_event))
            .route("/student/attendance",   web::get().to(student::student_attendance))
            .route("/student/forum",        web::get().to(student::student_forum))
            //.route("/student/home", web::get().to(student_home)) //to be removed

            // Lecturer Routes
            .route("/lecturer/dashboard", web::get().to(lecturer::lecturer_dashboard))
            .route("/lecturer/courses", web::get().to(lecturer::lecturer_courses_page))
            .route("/lecturer/assignments", web::get().to(lecturer::lecturer_assignments_page))
            .route("/lecturer/quizzes", web::get().to(lecturer::lecturer_quizzes_page))
            .route("/lecturer/grades", web::get().to(lecturer::lecturer_grades_page))
            .route("/lecturer/attendance", web::get().to(lecturer::lecturer_attendance_page))
            .route("/lecturer/forum", web::get().to(lecturer::lecturer_forum_page))
            .route("/lecturer/profile", web::get().to(lecturer::lecturer_profile_page))
            .route("/lecturer/settings", web::get().to(lecturer::lecturer_settings_page))

            // Admin Routes
            .route("/admin/dashboard", web::get().to(admin::admin_dashboard))
            .route("/admin/users", web::get().to(admin::admin_users_page))
            .route("/admin/users/create", web::post().to(admin::admin_create_user))
            .route("/admin/courses", web::get().to(admin::admin_courses_page))
            .route("/admin/content", web::get().to(admin::admin_content_page))
            .route("/admin/settings", web::get().to(admin::admin_settings_page))
            .route("/admin/audit", web::get().to(admin::admin_audit_page))
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
