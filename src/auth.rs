use actix_session::Session;
use actix_web::{HttpResponse, http::header, web};
use argon2::{Argon2, PasswordHash, PasswordVerifier};
use serde::Deserialize;
use sqlx::{FromRow, PgPool};
use tera::{Context, Tera};

use crate::admin::{AuditActor, log_audit_event};

// Session keys used across the authentication flow.
const SESSION_USER_ID: &str = "user_id";
const SESSION_ROLE: &str = "role";
const SESSION_DISPLAY_NAME: &str = "display_name";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UserRole {
    Student,
    Lecturer,
    Admin,
}

impl UserRole {
    pub fn from_slug(slug: &str) -> Option<Self> {
        match slug {
            "student" => Some(Self::Student),
            "lecturer" => Some(Self::Lecturer),
            "admin" => Some(Self::Admin),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Student => "student",
            Self::Lecturer => "lecturer",
            Self::Admin => "admin",
        }
    }

    #[allow(dead_code)]
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Student => "Student",
            Self::Lecturer => "Lecturer",
            Self::Admin => "Admin",
        }
    }

    #[allow(dead_code)]
    pub fn login_path(self) -> &'static str {
        "/login"
    }

    pub fn home_path(self) -> &'static str {
        match self {
            Self::Student => "/student/dashboard",
            Self::Lecturer => "/lecturer/dashboard",
            Self::Admin => "/admin/dashboard",
        }
    }
}

#[derive(Debug, FromRow)]
struct User {
    id: i32,
    display_name: String,
    password_hash: String,
    role: String,
    is_active: bool,
    must_change_password: bool,
}

#[derive(Debug, Deserialize)]
pub struct LoginForm {
    email: String,
    password: String,
}

#[derive(Debug)]
pub struct CurrentUser {
    pub id: i32,
    pub role: String,
    pub display_name: String,
}

// Handle login requests, verify credentials, and create the user's session.
pub async fn login_submit(
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
    form: web::Form<LoginForm>,
) -> HttpResponse {
    let email = form.email.trim().to_lowercase();
    if email.is_empty() || form.password.is_empty() {
        return render_login(
            tmpl.get_ref(),
            &email,
            "Email and password are required.",
            true,
        );
    }

    let user = match find_user_by_email(db.get_ref(), &email).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            return render_login(tmpl.get_ref(), &email, "Invalid email or password.", true);
        }
        Err(error) => {
            return HttpResponse::InternalServerError()
                .body(format!("Failed to check login details: {error}"));
        }
    };

    if !user.is_active {
        return render_login(
            tmpl.get_ref(),
            &email,
            "This account is currently inactive.",
            true,
        );
    }

    if !verify_password(&form.password, &user.password_hash) {
        return render_login(tmpl.get_ref(), &email, "Invalid email or password.", true);
    }

    session.renew();
    if let Err(error) = store_session(&session, &user) {
        return HttpResponse::InternalServerError()
            .body(format!("Failed to create login session: {error}"));
    }

    let actor = AuditActor {
        user_id: Some(user.id),
        role: Some(user.role.clone()),
        display_name: Some(user.display_name.clone()),
    };
    log_audit_event(
        db.get_ref(),
        "auth",
        "login_success",
        "info",
        &actor,
        Some("session"),
        Some(user.id),
        Some(format!("Signed in as {}", user.role)),
    )
    .await;

    if user.must_change_password {
        // keep session, but force password change
        return redirect("/password/change");
    }

    // Fetch user's saved theme (default to light) and set a cookie so pages can apply it immediately
    let theme_mode: String =
        sqlx::query_scalar("SELECT theme_mode FROM user_preferences WHERE user_id = $1")
            .bind(user.id)
            .fetch_optional(db.get_ref())
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| "light".to_string());

    let Some(role) = UserRole::from_slug(&user.role) else {
        return HttpResponse::InternalServerError().body("Unknown user role");
    };

    let location = role.home_path();
    let mut builder = HttpResponse::SeeOther();
    builder.insert_header((header::LOCATION, location));
    builder.insert_header((header::CACHE_CONTROL, "no-store"));

    let cookie_val = format!(
        "lms-theme={}; Path=/; Max-Age=31536000; SameSite=Lax",
        theme_mode
    );
    builder.insert_header((header::SET_COOKIE, cookie_val));

    builder.finish()
}

pub async fn logout(db: web::Data<PgPool>, session: Session) -> HttpResponse {
    let actor = current_user(&session)
        .ok()
        .flatten()
        .map(|user| AuditActor {
            user_id: Some(user.id),
            role: Some(user.role),
            display_name: Some(user.display_name),
        });
    if let Some(actor) = actor.as_ref() {
        log_audit_event(
            db.get_ref(),
            "auth",
            "logout",
            "info",
            actor,
            Some("session"),
            actor.user_id,
            Some("Signed out".to_string()),
        )
        .await;
    }
    session.purge();
    HttpResponse::SeeOther()
        .insert_header((header::LOCATION, "/?logged_out=1"))
        .insert_header((
            header::SET_COOKIE,
            "lms-theme=; Path=/; Max-Age=0; SameSite=Lax",
        ))
        .finish()
}

pub fn redirect_authenticated_user(
    session: &Session,
) -> Result<Option<HttpResponse>, HttpResponse> {
    let Some(current_user) = current_user(session)? else {
        return Ok(None);
    };

    let Some(role) = UserRole::from_slug(&current_user.role) else {
        return Ok(None);
    };

    Ok(Some(redirect(role.home_path())))
}

pub fn require_role(
    session: &Session,
    required_role: UserRole,
) -> Result<CurrentUser, HttpResponse> {
    let Some(current_user) = current_user(session)? else {
        return Err(redirect(required_role.login_path()));
    };

    if current_user.role != required_role.as_str() {
        return Err(HttpResponse::Forbidden()
            .content_type("text/plain")
            .body("You do not have permission to access this page."));
    }

    Ok(current_user)
}

fn render_login(
    tmpl: &Tera,
    email_value: &str,
    error_message: &str,
    has_error: bool,
) -> HttpResponse {
    let mut ctx = Context::new();
    ctx.insert("email_value", email_value);
    ctx.insert("error_message", error_message);
    ctx.insert("has_error", &has_error);

    match tmpl.render("index.html", &ctx) {
        Ok(rendered) => HttpResponse::Ok()
            .insert_header((header::CACHE_CONTROL, "no-store"))
            .content_type("text/html")
            .body(rendered),
        Err(error) => HttpResponse::InternalServerError()
            .body(format!("Failed to render login page: {error}")),
    }
}

async fn find_user_by_email(db: &PgPool, email: &str) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as::<_, User>(
        "SELECT id, display_name, password_hash, role, is_active
            , must_change_password
            FROM users
         WHERE LOWER(email) = $1",
    )
    .bind(email)
    .fetch_optional(db)
    .await
}

fn verify_password(password: &str, stored_hash: &str) -> bool {
    let Ok(parsed_hash) = PasswordHash::new(stored_hash) else {
        return false;
    };

    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok()
}

fn store_session(session: &Session, user: &User) -> Result<(), String> {
    session
        .insert(SESSION_USER_ID, user.id)
        .map_err(|error| error.to_string())?;
    session
        .insert(SESSION_ROLE, &user.role)
        .map_err(|error| error.to_string())?;
    session
        .insert(SESSION_DISPLAY_NAME, &user.display_name)
        .map_err(|error| error.to_string())?;

    Ok(())
}

fn current_user(session: &Session) -> Result<Option<CurrentUser>, HttpResponse> {
    let user_id = session
        .get::<i32>(SESSION_USER_ID)
        .map_err(session_error_response)?;
    let role = session
        .get::<String>(SESSION_ROLE)
        .map_err(session_error_response)?;
    let display_name = session
        .get::<String>(SESSION_DISPLAY_NAME)
        .map_err(session_error_response)?;

    let Some(user_id) = user_id else {
        return Ok(None);
    };
    let Some(role) = role else {
        return Ok(None);
    };
    let Some(display_name) = display_name else {
        return Ok(None);
    };

    Ok(Some(CurrentUser {
        id: user_id,
        role,
        display_name,
    }))
}

fn session_error_response(error: actix_session::SessionGetError) -> HttpResponse {
    HttpResponse::InternalServerError().body(format!("Failed to read login session: {error}"))
}

fn redirect(location: &str) -> HttpResponse {
    HttpResponse::SeeOther()
        .insert_header((header::LOCATION, location))
        .insert_header((header::CACHE_CONTROL, "no-store"))
        .finish()
}
