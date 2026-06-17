use actix_session::Session;
use actix_web::{web, HttpResponse, Responder};
use tera::{Context, Tera};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::PgPool;
use serde_json::json;

use crate::auth::UserRole;

#[derive(Deserialize, Clone)]
pub struct AdminCreateUserForm {
    pub display_name: String,
    pub email: String,
    pub role: String,
    pub age: Option<String>,
    pub programme: Option<String>,
    pub year_of_study: Option<String>,
    pub staff_no: Option<String>,
    pub department: Option<String>,
}

#[derive(Serialize, FromRow)]
pub struct AdminUserListItem {
    pub id: i32,
    pub display_name: String,
    pub email: String,
    pub role: String,
    pub is_active: bool,
    pub must_change_password: bool,
    pub created_at_iso: String,
    pub created_at: String,
}

fn set_admin_user_page_base(ctx: &mut Context, user: &crate::auth::CurrentUser) {
    ctx.insert("display_name", &user.display_name);
    ctx.insert("student_name", &user.display_name);
    ctx.insert("student_id", "");
    ctx.insert("notifications", &Vec::<crate::NotificationContext>::new());
    ctx.insert("active_page", "users");
    ctx.insert("is_admin", &true);
}

fn set_admin_user_form_defaults(ctx: &mut Context) {
    ctx.insert("form_display_name", "");
    ctx.insert("form_email", "");
    ctx.insert("form_role", "student");
    ctx.insert("form_age", "");
    ctx.insert("form_programme", "");
    ctx.insert("form_year_of_study", "");
    ctx.insert("form_staff_no", "");
    ctx.insert("form_department", "");
}

fn set_admin_user_form_values(ctx: &mut Context, form: &AdminCreateUserForm) {
    ctx.insert("form_display_name", form.display_name.trim());
    ctx.insert("form_email", form.email.trim());
    ctx.insert("form_role", form.role.trim());
    ctx.insert("form_age", form.age.as_deref().unwrap_or("").trim());
    ctx.insert(
        "form_programme",
        form.programme.as_deref().unwrap_or("").trim(),
    );
    ctx.insert(
        "form_year_of_study",
        form.year_of_study.as_deref().unwrap_or("").trim(),
    );
    ctx.insert("form_staff_no", form.staff_no.as_deref().unwrap_or("").trim());
    ctx.insert(
        "form_department",
        form.department.as_deref().unwrap_or("").trim(),
    );
}

pub async fn admin_users_page(
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Admin) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    set_admin_user_page_base(&mut ctx, &user);
    set_admin_user_form_defaults(&mut ctx);
    ctx.insert("form_action", "/admin/users/create");

    let users = sqlx::query_as::<_, AdminUserListItem>(
        r#"SELECT id, display_name, email, role, is_active
         , must_change_password
         , to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS"Z"') as created_at_iso
         , to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS') as created_at
         FROM users
         ORDER BY created_at DESC, id DESC
         LIMIT 200"#,
    )
    .fetch_all(db.get_ref())
    .await;

    match users {
        Ok(rows) => {
            let total_users = rows.len();
            let active_users = rows.iter().filter(|user| user.is_active).count();
            let student_users = rows.iter().filter(|user| user.role == "student").count();
            let lecturer_users = rows.iter().filter(|user| user.role == "lecturer").count();

            ctx.insert("total_users", &total_users);
            ctx.insert("active_users", &active_users);
            ctx.insert("student_users", &student_users);
            ctx.insert("lecturer_users", &lecturer_users);
            ctx.insert("users", &rows);
        }
        Err(_) => {
            ctx.insert("total_users", &0usize);
            ctx.insert("active_users", &0usize);
            ctx.insert("student_users", &0usize);
            ctx.insert("lecturer_users", &0usize);
            ctx.insert("users", &Vec::<AdminUserListItem>::new());
        }
    }

    // Pull any one-time success/temp password from session (set after PRG) and clear them
    if let Ok(Some(msg)) = session.get::<String>("create_success") {
        ctx.insert("create_success", &msg);
        let _ = session.remove("create_success");
    }
    if let Ok(Some(tmp)) = session.get::<String>("temp_password") {
        ctx.insert("temp_password", &tmp);
        let _ = session.remove("temp_password");
    }
    if let Ok(Some(msg)) = session.get::<String>("user_status_success") {
        ctx.insert("user_status_success", &msg);
        let _ = session.remove("user_status_success");
    }
    if let Ok(Some(msg)) = session.get::<String>("user_status_error") {
        ctx.insert("user_status_error", &msg);
        let _ = session.remove("user_status_error");
    }

    let rendered = match tmpl.render("admin/user_management.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

pub async fn admin_create_user(
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
    form: web::Form<AdminCreateUserForm>,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Admin) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let form = form.into_inner();
    let role = form.role.trim().to_lowercase();
    let display_name = form.display_name.trim();
    let email = form.email.trim().to_lowercase();

    let mut ctx = Context::new();
    set_admin_user_page_base(&mut ctx, &user);
    set_admin_user_form_values(&mut ctx, &form);
    ctx.insert("form_action", "/admin/users/create");

    let mut validation_error: Option<String> = None;
    if display_name.is_empty() || email.is_empty() {
        validation_error = Some("Display name and email are required.".to_string());
    } else if role != "student" && role != "lecturer" {
        validation_error = Some("Role must be either student or lecturer.".to_string());
    }

    let age = match crate::parse_optional_i32(form.age.as_deref(), "Age") {
        Ok(value) => value,
        Err(message) => {
            validation_error = Some(message);
            None
        }
    };
    let year_of_study = match crate::parse_optional_i32(form.year_of_study.as_deref(), "Year of study") {
        Ok(value) => value,
        Err(message) => {
            validation_error = Some(message);
            None
        }
    };

    if validation_error.is_none() {
        if let Some(year) = year_of_study {
            if !(1..=4).contains(&year) {
                validation_error = Some("Year of study must be 1, 2, 3, or 4.".to_string());
            }
        }
    }

    if validation_error.is_none() && role == "student" {
        if form.programme.as_deref().unwrap_or("").trim().is_empty() {
            validation_error = Some("Programme is required for students.".to_string());
        } else if year_of_study.is_none() {
            validation_error = Some("Year of study is required for students.".to_string());
        }
    }

    if validation_error.is_none() && role == "lecturer" {
        if form.staff_no.as_deref().unwrap_or("").trim().is_empty()
            || form.department.as_deref().unwrap_or("").trim().is_empty()
        {
            validation_error = Some("Staff number and department are required for lecturers.".to_string());
        }
    }

    if let Some(message) = validation_error {
        ctx.insert("create_error", &message);
        ctx.insert("users", &Vec::<AdminUserListItem>::new());
        let rendered = match tmpl.render("admin/user_management.html", &ctx) {
            Ok(html) => html,
            Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
        };
        return HttpResponse::BadRequest().content_type("text/html").body(rendered);
    }
    
    // Always generate a temporary password (admin does not supply it)
    let tmp = crate::generate_temp_password(12);
    let temp_password = Some(tmp.clone());
    let password_to_hash = tmp;

    let password_hash = match crate::hash_password(&password_to_hash) {
        Ok(hash) => hash,
        Err(error) => {
            ctx.insert("create_error", &error);
            ctx.insert("users", &Vec::<AdminUserListItem>::new());
            let rendered = match tmpl.render("admin/user_management.html", &ctx) {
                Ok(html) => html,
                Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
            };
            return HttpResponse::InternalServerError().content_type("text/html").body(rendered);
        }
    };

    let mut tx = match db.begin().await {
        Ok(transaction) => transaction,
        Err(error) => {
            return HttpResponse::InternalServerError()
                .body(format!("Failed to start DB transaction: {error}"));
        }
    };

    let must_change = temp_password.is_some();

    let user_id_result = sqlx::query_scalar(
        "INSERT INTO users (display_name, email, password_hash, role, is_active, must_change_password)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id",
    )
    .bind(display_name)
    .bind(&email)
    .bind(&password_hash)
    .bind(&role)
    .bind(true)
    .bind(must_change)
    .fetch_one(&mut *tx)
    .await;

    let user_id: i32 = match user_id_result {
        Ok(id) => id,
        Err(error) => {
            let _ = tx.rollback().await;
            ctx.insert("create_error", &format!("Failed to create user account: {error}"));
                ctx.insert("users", &Vec::<AdminUserListItem>::new());
            let rendered = match tmpl.render("admin/user_management.html", &ctx) {
                Ok(html) => html,
                Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
            };
            return HttpResponse::BadRequest().content_type("text/html").body(rendered);
        }
    };

    let profile_result = if role == "student" {
        sqlx::query(
            "INSERT INTO students (user_id, age, programme, year_of_study)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(user_id)
        .bind(age)
        .bind(form.programme.as_deref().map(str::trim).filter(|v| !v.is_empty()))
        .bind(year_of_study)
        .execute(&mut *tx)
        .await
    } else {
        sqlx::query(
            "INSERT INTO lecturers (user_id, staff_no, department)
             VALUES ($1, $2, $3)",
        )
        .bind(user_id)
        .bind(form.staff_no.as_deref().unwrap_or("").trim())
        .bind(form.department.as_deref().unwrap_or("").trim())
        .execute(&mut *tx)
        .await
    };

    if let Err(error) = profile_result {
        let _ = tx.rollback().await;
        ctx.insert(
            "create_error",
            &format!("Failed to create {} profile: {error}", role),
        );
        ctx.insert("users", &Vec::<AdminUserListItem>::new());
        let rendered = match tmpl.render("admin/user_management.html", &ctx) {
            Ok(html) => html,
            Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
        };
        return HttpResponse::BadRequest().content_type("text/html").body(rendered);
    }

    if let Err(error) = tx.commit().await {
        return HttpResponse::InternalServerError()
            .body(format!("Failed to commit DB transaction: {error}"));
    }

    let _users = sqlx::query_as::<_, AdminUserListItem>(
        r#"SELECT id, display_name, email, role, is_active, must_change_password,
         to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS"Z"') as created_at_iso,
         to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS') as created_at
         FROM users
         ORDER BY created_at DESC, id DESC
         LIMIT 200"#,
    )
    .fetch_all(db.get_ref())
    .await
    .unwrap_or_default();

    // Use Post-Redirect-Get: store one-time success/temp password in session, then redirect.
    if let Some(tmp) = temp_password {
        if let Err(error) = session.insert("temp_password", &tmp) {
            return HttpResponse::InternalServerError().body(format!("Failed to set session: {error}"));
        }
    }

    let success_msg = format!("Created {role} account for {display_name}.");
    if let Err(error) = session.insert("create_success", &success_msg) {
        return HttpResponse::InternalServerError().body(format!("Failed to set session: {error}"));
    }

    // Redirect to the users page (GET) to avoid form resubmission on reload
    HttpResponse::SeeOther()
        .insert_header((actix_web::http::header::LOCATION, "/admin/users"))
        .finish()
}

pub async fn admin_toggle_user_active(
    db: web::Data<PgPool>,
    session: Session,
    user_id: web::Path<i32>,
) -> impl Responder {
    let current_user = match crate::auth::require_role(&session, UserRole::Admin) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let user_id = user_id.into_inner();
    if user_id == current_user.id {
        let _ = session.insert(
            "user_status_error",
            "You cannot deactivate your own admin account.",
        );
        return HttpResponse::SeeOther()
            .insert_header((actix_web::http::header::LOCATION, "/admin/users"))
            .finish();
    }

    let user = sqlx::query_as::<_, AdminUserListItem>(
        r#"SELECT id, display_name, email, role, is_active
         , must_change_password
         , to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS"Z"') as created_at_iso
         , to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS') as created_at
         FROM users
         WHERE id = $1"#,
    )
    .bind(user_id)
    .fetch_optional(db.get_ref())
    .await;

    let user = match user {
        Ok(Some(user)) => user,
        Ok(None) => {
            let _ = session.insert("user_status_error", "User account not found.");
            return HttpResponse::SeeOther()
                .insert_header((actix_web::http::header::LOCATION, "/admin/users"))
                .finish();
        }
        Err(error) => {
            return HttpResponse::InternalServerError()
                .body(format!("Failed to load user account: {error}"));
        }
    };

    let new_status = !user.is_active;
    if let Err(error) = sqlx::query("UPDATE users SET is_active = $1, updated_at = NOW() WHERE id = $2")
        .bind(new_status)
        .bind(user.id)
        .execute(db.get_ref())
        .await
    {
        return HttpResponse::InternalServerError()
            .body(format!("Failed to update user account status: {error}"));
    }

    let action = if new_status { "activated" } else { "deactivated" };
    let message = format!("{} has been {action}.", user.display_name);
    let _ = session.insert("user_status_success", &message);

    HttpResponse::SeeOther()
        .insert_header((actix_web::http::header::LOCATION, "/admin/users"))
        .finish()
}

pub async fn admin_courses_page(
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Admin) {
        Ok(user) => user,
        Err(response) => return response,
    };

    #[derive(Serialize)]
    struct CourseRow {
        id: i32,
        code: String,
        name: String,
        status: String,
        trimester: String,
        lecturer_name: String,  // "Unassigned" if NULL
    }

    let courses = sqlx::query_as!(
        CourseRow,
        r#"SELECT
            c.id,
            c.course_code   AS code,
            c.course_name   AS name,
            COALESCE(c.status, 'Preparing')     AS "status!",
            COALESCE(c.trimester, '')            AS "trimester!",
            COALESCE(u.display_name, 'Unassigned') AS "lecturer_name!"
           FROM courses c
           LEFT JOIN lecturers l ON l.id = c.lecturer_id
           LEFT JOIN users u     ON u.id = l.user_id
           ORDER BY c.created_at DESC"#
    )
    .fetch_all(db.get_ref())
    .await
    .unwrap_or_default();

    // Fetch all lecturers for the assign dropdown
    #[derive(Serialize)]
    struct LecturerOption {
        id: i32,
        name: String,
    }

    let lecturers = sqlx::query_as!(
        LecturerOption,
        r#"SELECT l.id, u.display_name AS name
           FROM lecturers l
           JOIN users u ON u.id = l.user_id
           ORDER BY u.display_name"#
    )
    .fetch_all(db.get_ref())
    .await
    .unwrap_or_default();

    let mut ctx = Context::new();
    ctx.insert("display_name", &user.display_name);
    ctx.insert("student_name", &user.display_name);
    ctx.insert("student_id", "");
    ctx.insert("notifications", &Vec::<crate::NotificationContext>::new());
    ctx.insert("active_page", "courses");
    ctx.insert("is_admin", &true);
    ctx.insert("courses", &courses);
    ctx.insert("lecturers", &lecturers);  // for the assign dropdown

    let rendered = match tmpl.render("admin/course_administration.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

pub async fn admin_content_page(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Admin) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    ctx.insert("display_name", &user.display_name);
    ctx.insert("student_name", &user.display_name);
    ctx.insert("student_id", "");
    ctx.insert("notifications", &Vec::<crate::NotificationContext>::new());
    ctx.insert("active_page", "content");
    ctx.insert("is_admin", &true);

    let rendered = match tmpl.render("admin/content_oversight.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

pub async fn admin_settings_page(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Admin) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    ctx.insert("display_name", &user.display_name);
    ctx.insert("student_name", &user.display_name);
    ctx.insert("student_id", "");
    ctx.insert("notifications", &Vec::<crate::NotificationContext>::new());
    ctx.insert("active_page", "settings");
    ctx.insert("is_admin", &true);

    let rendered = match tmpl.render("admin/global_settings.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

pub async fn admin_audit_page(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Admin) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    ctx.insert("display_name", &user.display_name);
    ctx.insert("student_name", &user.display_name);
    ctx.insert("student_id", "");
    ctx.insert("notifications", &Vec::<crate::NotificationContext>::new());
    ctx.insert("active_page", "audit");
    ctx.insert("is_admin", &true);

    let rendered = match tmpl.render("admin/security_audit.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

async fn fetch_count(db: &PgPool, sql: &'static str) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar::<_, i64>(sql)
        .fetch_one(db)
        .await
}

pub async fn admin_dashboard(
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Admin) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    // Provide display name used in admin template
    ctx.insert("display_name", &user.display_name);
    // Navbar expects these student-specific variables; provide admin-friendly values
    ctx.insert("student_name", &user.display_name);
    ctx.insert("student_id", "");
    // No notifications for now
    let notifications: Vec<crate::NotificationContext> = vec![];
    ctx.insert("notifications", &notifications);
    // Highlight active sidebar item
    ctx.insert("active_page", "dashboard");
    // Mark template as admin so shared partials can adapt
    ctx.insert("is_admin", &true);
    let students_count = match fetch_count(db.get_ref(), "SELECT COUNT(*) FROM students").await {
        Ok(count) => count,
        Err(error) => {
            return HttpResponse::InternalServerError()
                .body(format!("Failed to load dashboard statistics: {error}"));
        }
    };
    let lecturers_count = match fetch_count(db.get_ref(), "SELECT COUNT(*) FROM lecturers").await {
        Ok(count) => count,
        Err(error) => {
            return HttpResponse::InternalServerError()
                .body(format!("Failed to load dashboard statistics: {error}"));
        }
    };
    let admins_count = match fetch_count(db.get_ref(), "SELECT COUNT(*) FROM users WHERE role = 'admin'").await {
        Ok(count) => count,
        Err(error) => {
            return HttpResponse::InternalServerError()
                .body(format!("Failed to load dashboard statistics: {error}"));
        }
    };
    let courses_count = match fetch_count(db.get_ref(), "SELECT COUNT(*) FROM courses").await {
        Ok(count) => count,
        Err(error) => {
            return HttpResponse::InternalServerError()
                .body(format!("Failed to load dashboard statistics: {error}"));
        }
    };
    let enrollments_count =
        match fetch_count(db.get_ref(), "SELECT COUNT(*) FROM enrollments").await {
            Ok(count) => count,
            Err(error) => {
                return HttpResponse::InternalServerError()
                    .body(format!("Failed to load dashboard statistics: {error}"));
            }
        };

    ctx.insert("students_count", &students_count);
    ctx.insert("lecturers_count", &lecturers_count);
    ctx.insert("admins_count", &admins_count);
    ctx.insert("courses_count", &courses_count);
    ctx.insert("enrollments_count", &enrollments_count);
    // Recent activity placeholder list (hardcoded sample events)
    #[derive(Serialize)]
    struct Activity {
        who: String,
        action: String,
        when: String,
    }
    let recent_activity: Vec<Activity> = vec![
        Activity { who: "alice@student.test".into(), action: "created student account".into(), when: "10m ago".into() },
        Activity { who: "bob@lecturer.test".into(), action: "published announcement".into(), when: "30m ago".into() },
        Activity { who: "system".into(), action: "daily enrollment sync".into(), when: "1h ago".into() },
    ];
    ctx.insert("recent_activity", &recent_activity);

    // Content preview cards (hardcoded for now; replace with DB query later)
    #[derive(Serialize)]
    struct ContentPreview {
        author: String,
        kind: String,
        title: String,
        snippet: String,
        when: String,
    }
    let content_previews: Vec<ContentPreview> = vec![
        ContentPreview {
            author: "Dr. Tan Wei Ming".into(),
            kind: "Announcement".into(),
            title: "Assignment 2 brief released".into(),
            snippet: "The brief for Assignment 2 is now available. Students should review the submission requirements and deadline.".into(),
            when: "24 May 2026".into(),
        },
        ContentPreview {
            author: "Aisha Rahman".into(),
            kind: "Forum Post".into(),
            title: "Questions about lab setup".into(),
            snippet: "Has anyone managed to configure the local environment on Windows without Docker issues?".into(),
            when: "23 May 2026".into(),
        },
        ContentPreview {
            author: "Mr. Lim".into(),
            kind: "Uploaded Material".into(),
            title: "Week 8 lecture slides".into(),
            snippet: "Slides for the upcoming lecture have been uploaded and include examples for the revision session.".into(),
            when: "22 May 2026".into(),
        },
    ];
    ctx.insert("content_previews", &content_previews);

    let rendered = match tmpl.render("admin/dashboard.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };

    HttpResponse::Ok().content_type("text/html").body(rendered)
}

#[derive(serde::Deserialize)]
pub struct CreateCourseForm {
    pub course_code: String,
    pub course_name: String,
    pub description: Option<String>,
    pub trimester: Option<String>,
    pub max_students: Option<i32>,
    pub lecturer_id: Option<i32>,
}

pub async fn create_course(
    form: web::Json<CreateCourseForm>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    let _user = match crate::auth::require_role(&session, UserRole::Admin) {
        Ok(u) => u,
        Err(r) => return r,
    };

    let result = sqlx::query!(
    "INSERT INTO courses (course_code, course_name, description, trimester, max_students, lecturer_id)
     VALUES ($1, $2, $3, $4, $5, $6)",
    form.course_code,
    form.course_name,
    form.description.as_deref().unwrap_or(""),
    form.trimester.as_deref().unwrap_or(""),
    form.max_students,
    form.lecturer_id,
)
    .execute(db.get_ref())
    .await;

    match result {
        Ok(_) => HttpResponse::Ok().json(json!({ "message": "Course created" })),
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
    }
}

#[derive(serde::Deserialize)]
pub struct AssignLecturerForm {
    pub lecturer_id: i32,
}

pub async fn assign_lecturer(
    cid: web::Path<i32>,
    form: web::Json<AssignLecturerForm>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    let _user = match crate::auth::require_role(&session, UserRole::Admin) {
        Ok(u) => u,
        Err(r) => return r,
    };

    let result = sqlx::query!(
        "UPDATE courses SET lecturer_id = $1 WHERE id = $2",
        form.lecturer_id,
        cid.into_inner()
    )
    .execute(db.get_ref())
    .await;

    match result {
        Ok(_) => HttpResponse::Ok().json(json!({ "message": "Lecturer assigned" })),
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
    }
}

pub async fn delete_course(
    cid: web::Path<i32>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    let _user = match crate::auth::require_role(&session, UserRole::Admin) {
        Ok(u) => u,
        Err(r) => return r,
    };

    let course_id = cid.into_inner();

    // Delete uploaded files
    let _ = std::fs::remove_dir_all(format!("uploads/courses/{}", course_id));

    let result = sqlx::query!(
        "DELETE FROM courses WHERE id = $1",
        course_id
    )
    .execute(db.get_ref())
    .await;

    match result {
        Ok(_) => HttpResponse::Ok().json(json!({ "message": "Course deleted" })),
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
    }
}

// ─── GET enrolled students for a course ──────────────────────────────────────
pub async fn get_course_enrollments(
    cid: web::Path<i32>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    match crate::auth::require_role(&session, crate::auth::UserRole::Admin) {
        Ok(_) => {}
        Err(r) => return r,
    }
    let course_id = cid.into_inner();

    let enrolled = sqlx::query!(
        r#"SELECT s.id AS student_id, u.display_name, u.email
           FROM enrollments e
           JOIN students s ON s.id = e.student_id
           JOIN users u ON u.id = s.user_id
           WHERE e.course_id = $1
           ORDER BY u.display_name ASC"#,
        course_id
    )
    .fetch_all(db.get_ref())
    .await;

    let all_students = sqlx::query!(
        r#"SELECT s.id AS student_id, u.display_name, u.email
           FROM students s
           JOIN users u ON u.id = s.user_id
           ORDER BY u.display_name ASC"#
    )
    .fetch_all(db.get_ref())
    .await;

    match (enrolled, all_students) {
        (Ok(enrolled), Ok(all)) => {
            let enrolled_ids: Vec<i32> = enrolled.iter().map(|r| r.student_id).collect();
            let all_list: Vec<serde_json::Value> = all.iter().map(|r| {
                json!({
                    "student_id": r.student_id,
                    "display_name": r.display_name,
                    "email": r.email,
                    "enrolled": enrolled_ids.contains(&r.student_id)
                })
            }).collect();
            HttpResponse::Ok().json(json!({ "students": all_list }))
        }
        _ => HttpResponse::InternalServerError().body("Failed to load enrollment data"),
    }
}

// ─── ENROLL a student into a course ──────────────────────────────────────────
#[derive(serde::Deserialize)]
pub struct EnrollForm {
    pub student_id: i32,
}

pub async fn enroll_student(
    cid: web::Path<i32>,
    form: web::Json<EnrollForm>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    match crate::auth::require_role(&session, crate::auth::UserRole::Admin) {
        Ok(_) => {}
        Err(r) => return r,
    }
    let course_id = cid.into_inner();

    let result = sqlx::query!(
        "INSERT INTO enrollments (student_id, course_id)
         VALUES ($1, $2)
         ON CONFLICT (student_id, course_id) DO NOTHING",
        form.student_id, course_id
    )
    .execute(db.get_ref())
    .await;

    match result {
        Ok(_)  => HttpResponse::Ok().json(json!({ "message": "Student enrolled" })),
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
    }
}

// ─── UNENROLL a student from a course ────────────────────────────────────────
#[derive(serde::Deserialize)]
pub struct UnenrollForm {
    pub student_id: i32,
}

pub async fn unenroll_student(
    cid: web::Path<i32>,
    form: web::Json<UnenrollForm>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    match crate::auth::require_role(&session, crate::auth::UserRole::Admin) {
        Ok(_) => {}
        Err(r) => return r,
    }
    let course_id = cid.into_inner();

    let result = sqlx::query!(
        "DELETE FROM enrollments WHERE student_id = $1 AND course_id = $2",
        form.student_id, course_id
    )
    .execute(db.get_ref())
    .await;

    match result {
        Ok(_)  => HttpResponse::Ok().json(json!({ "message": "Student unenrolled" })),
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
    }
}
