use actix_files::Files;
use actix_session::{Session, SessionMiddleware, storage::CookieSessionStore};
use actix_web::{App, HttpResponse, HttpServer, Responder, cookie::Key, web};
use tera::{Context, Tera};

use dotenvy::dotenv;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool, postgres::PgPoolOptions};
use std::env;

mod auth;

use auth::UserRole;

#[derive(Serialize, FromRow)]
struct Student {
    id: i32,
    name: String,
    email: String,
    age: Option<i32>,
}

#[derive(Deserialize)]
struct CreateStudent {
    name: String,
    email: String,
    age: Option<i32>,
}

async fn get_students(db: web::Data<PgPool>) -> impl Responder {
    let result =
        sqlx::query_as::<_, Student>("SELECT id, name, email, age FROM students ORDER BY id")
            .fetch_all(db.get_ref())
            .await;

    match result {
        Ok(students) => HttpResponse::Ok().json(students),
        Err(error) => HttpResponse::InternalServerError().body(error.to_string()),
    }
}

async fn create_student(
    db: web::Data<PgPool>,
    student: web::Json<CreateStudent>,
) -> impl Responder {
    let result = sqlx::query_as::<_, Student>(
        "INSERT INTO students (name, email, age)
         VALUES ($1, $2, $3)
         RETURNING id, name, email, age",
    )
    .bind(&student.name)
    .bind(&student.email)
    .bind(student.age)
    .fetch_one(db.get_ref())
    .await;

    match result {
        Ok(new_student) => HttpResponse::Ok().json(new_student),
        Err(error) => HttpResponse::InternalServerError().body(error.to_string()),
    }
}

async fn index(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    match auth::redirect_authenticated_user(&session) {
        Ok(Some(response)) => return response,
        Ok(None) => {}
        Err(response) => return response,
    }

    let ctx = Context::new();
    let rendered = tmpl.render("index.html", &ctx).unwrap();

    HttpResponse::Ok().content_type("text/html").body(rendered)
}

async fn student_home(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match auth::require_role(&session, UserRole::Student) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    ctx.insert("display_name", &user.display_name);
    let rendered = tmpl.render("student_home.html", &ctx).unwrap();
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

async fn lecturer_home(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    ctx.insert("display_name", &user.display_name);
    let rendered = tmpl.render("lecturer_home.html", &ctx).unwrap();
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

async fn admin_home(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match auth::require_role(&session, UserRole::Admin) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    ctx.insert("display_name", &user.display_name);
    let rendered = tmpl.render("admin_home.html", &ctx).unwrap();
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

fn session_key() -> Key {
    let secret =
        env::var("SESSION_SECRET").expect("SESSION_SECRET must be set in .env for login sessions");

    if secret.as_bytes().len() < 64 {
        panic!("SESSION_SECRET must be at least 64 bytes long");
    }

    Key::from(secret.as_bytes())
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();

    let tera = Tera::new("templates/**/*").unwrap();
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set in .env");

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to connect to PostgreSQL");

    let session_key = session_key();

    println!("Connected to server");

    HttpServer::new(move || {
        App::new()
            .wrap(
                SessionMiddleware::builder(CookieSessionStore::default(), session_key.clone())
                    .cookie_secure(false)
                    .build(),
            )
            .app_data(web::Data::new(tera.clone()))
            .app_data(web::Data::new(pool.clone()))
            .service(Files::new("/static", "./static"))
            .route("/", web::get().to(index))
            .route("/login/{role}", web::get().to(auth::login_page))
            .route("/login/{role}", web::post().to(auth::login_submit))
            .route("/logout", web::post().to(auth::logout))
            .route("/student/home", web::get().to(student_home))
            .route("/lecturer/home", web::get().to(lecturer_home))
            .route("/admin/home", web::get().to(admin_home))
            .route("/api/students", web::get().to(get_students))
            .route("/api/students", web::post().to(create_student))
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
