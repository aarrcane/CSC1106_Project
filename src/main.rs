use actix_files::Files;
use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use tera::{Context, Tera};

use dotenvy::dotenv;
use serde::{Deserialize,Serialize};
use sqlx::{postgres::PgPoolOptions, FromRow, PgPool};
use std::env;


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
    let result = sqlx::query_as::<_, Student>(
        "SELECT id, name, email, age FROM students ORDER BY id"
    )
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
         RETURNING id, name, email, age"
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

async fn index(tmpl: web::Data<Tera>) -> impl Responder {
    let ctx = Context::new();
    let rendered = tmpl.render("index.html", &ctx).unwrap();

    HttpResponse::Ok()
        .content_type("text/html")
        .body(rendered)
}

//uncomment the below once user authentication is implemented

// async fn student_login_page(tmpl: web::Data<Tera>) -> impl Responder {
//     let mut ctx = Context::new();
//     ctx.insert("role_name", "Student");
//     ctx.insert("username_label", "Student ID");
//     ctx.insert("action_url", "/login/student");
//     let rendered = tmpl.render("login.html", &ctx).unwrap();

//     HttpResponse::Ok()
//         .content_type("text/html")
//         .body(rendered)
// }

// async fn lecturer_login_page(tmpl: web::Data<Tera>) -> impl Responder {
//     let mut ctx = Context::new();
//     ctx.insert("role_name", "Lecturer");
//     ctx.insert("username_label", "Email");
//     ctx.insert("action_url", "/login/lecturer");
//     let rendered = tmpl.render("login.html", &ctx).unwrap();

//     HttpResponse::Ok()
//         .content_type("text/html")
//         .body(rendered)
// }

// async fn admin_login_page(tmpl: web::Data<Tera>) -> impl Responder {
//     let mut ctx = Context::new();
//     ctx.insert("role_name", "Admin");
//     ctx.insert("username_label", "Email");
//     ctx.insert("action_url", "/login/admin");
//     let rendered = tmpl.render("login.html", &ctx).unwrap();

//     HttpResponse::Ok()
//         .content_type("text/html")
//         .body(rendered)
// }

// delete the below once user authentication is implemented

async fn student_login_page(tmpl: web::Data<Tera>) -> impl Responder {
    let mut ctx = Context::new();
    ctx.insert("role_name", "Student");
    ctx.insert("username_label", "Student ID");
    ctx.insert("action_url", "/student/home");
    let rendered = tmpl.render("login.html", &ctx).unwrap();

    HttpResponse::Ok().content_type("text/html").body(rendered)
}

async fn lecturer_login_page(tmpl: web::Data<Tera>) -> impl Responder {
    let mut ctx = Context::new();
    ctx.insert("role_name", "Lecturer");
    ctx.insert("username_label", "Email");
    ctx.insert("action_url", "/lecturer/home");
    let rendered = tmpl.render("login.html", &ctx).unwrap();

    HttpResponse::Ok().content_type("text/html").body(rendered)
}

async fn admin_login_page(tmpl: web::Data<Tera>) -> impl Responder {
    let mut ctx = Context::new();
    ctx.insert("role_name", "Admin");
    ctx.insert("username_label", "Email");
    ctx.insert("action_url", "/admin/home");
    let rendered = tmpl.render("login.html", &ctx).unwrap();

    HttpResponse::Ok().content_type("text/html").body(rendered)
}

async fn student_home(tmpl: web::Data<Tera>) -> impl Responder {
    let ctx = Context::new();
    let rendered = tmpl.render("student_home.html", &ctx).unwrap();
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

async fn lecturer_home(tmpl: web::Data<Tera>) -> impl Responder {
    let ctx = Context::new();
    let rendered = tmpl.render("lecturer_home.html", &ctx).unwrap();
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

async fn admin_home(tmpl: web::Data<Tera>) -> impl Responder {
    let ctx = Context::new();
    let rendered = tmpl.render("admin_home.html", &ctx).unwrap();
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();

    let tera = Tera::new("templates/**/*").unwrap();
    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set in .env");

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to connect to PostgreSQL");

    println!("Connected to server");

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(tera.clone()))
            .app_data(web::Data::new(pool.clone()))
            .service(Files::new("/static", "./static"))
            .route("/", web::get().to(index))
            .route("/login/student", web::get().to(student_login_page))
            .route("/login/lecturer", web::get().to(lecturer_login_page))
            .route("/login/admin", web::get().to(admin_login_page))
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