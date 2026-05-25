use actix_session::Session;
use actix_web::{HttpResponse, http::header, web};
use argon2::{Argon2, PasswordHash, PasswordVerifier};
use serde::Deserialize;
use sqlx::{FromRow, PgPool};
use tera::{Context, Tera};

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

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Student => "Student",
            Self::Lecturer => "Lecturer",
            Self::Admin => "Admin",
        }
    }

    pub fn login_path(self) -> &'static str {
        match self {
            Self::Student => "/login/student",
            Self::Lecturer => "/login/lecturer",
            Self::Admin => "/login/admin",
        }
    }

    pub fn home_path(self) -> &'static str {
        match self {
            Self::Student => "/student/home",
            Self::Lecturer => "/lecturer/home",
            Self::Admin => "/admin/home",
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
}

#[derive(Debug, Deserialize)]
pub struct LoginForm {
    email: String,
    password: String,
}

#[derive(Debug)]
pub struct CurrentUser {
    pub role: String,
    pub display_name: String,
}

pub async fn login_page(
    tmpl: web::Data<Tera>,
    session: Session,
    path: web::Path<String>,
) -> HttpResponse {
    match redirect_authenticated_user(&session) {
        Ok(Some(response)) => return response,
        Ok(None) => {}
        Err(response) => return response,
    }

    let Some(role) = UserRole::from_slug(path.as_str()) else {
        return HttpResponse::NotFound().body("Unknown login role");
    };

    render_login(tmpl.get_ref(), role, "", "", false)
}

pub async fn login_submit(
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
    path: web::Path<String>,
    form: web::Form<LoginForm>,
) -> HttpResponse {
    let Some(selected_role) = UserRole::from_slug(path.as_str()) else {
        return HttpResponse::NotFound().body("Unknown login role");
    };

    let email = form.email.trim().to_lowercase();
    if email.is_empty() || form.password.is_empty() {
        return render_login(
            tmpl.get_ref(),
            selected_role,
            &email,
            "Email and password are required.",
            true,
        );
    }

    let user = match find_user_by_email(db.get_ref(), &email).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            return render_login(
                tmpl.get_ref(),
                selected_role,
                &email,
                "Invalid email or password.",
                true,
            );
        }
        Err(error) => {
            return HttpResponse::InternalServerError()
                .body(format!("Failed to check login details: {error}"));
        }
    };

    if !user.is_active {
        return render_login(
            tmpl.get_ref(),
            selected_role,
            &email,
            "This account is currently inactive.",
            true,
        );
    }

    if !verify_password(&form.password, &user.password_hash) {
        return render_login(
            tmpl.get_ref(),
            selected_role,
            &email,
            "Invalid email or password.",
            true,
        );
    }

    if user.role != selected_role.as_str() {
        return render_login(
            tmpl.get_ref(),
            selected_role,
            &email,
            "This account does not have access to the selected portal.",
            true,
        );
    }

    session.renew();
    if let Err(error) = store_session(&session, &user) {
        return HttpResponse::InternalServerError()
            .body(format!("Failed to create login session: {error}"));
    }

    redirect(selected_role.home_path())
}

pub async fn logout(session: Session) -> HttpResponse {
    session.purge();
    redirect("/")
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
    role: UserRole,
    email_value: &str,
    error_message: &str,
    has_error: bool,
) -> HttpResponse {
    let mut ctx = Context::new();
    ctx.insert("role_name", role.display_name());
    ctx.insert("username_label", "Email");
    ctx.insert("action_url", role.login_path());
    ctx.insert("email_value", email_value);
    ctx.insert("error_message", error_message);
    ctx.insert("has_error", &has_error);

    match tmpl.render("login.html", &ctx) {
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

    if user_id.is_none() {
        return Ok(None);
    }
    let Some(role) = role else {
        return Ok(None);
    };
    let Some(display_name) = display_name else {
        return Ok(None);
    };

    Ok(Some(CurrentUser { role, display_name }))
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
