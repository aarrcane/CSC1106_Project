use actix_files::Files;
use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use tera::{Context, Tera};

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
    let tera = Tera::new("templates/**/*").unwrap();

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(tera.clone()))
            .service(Files::new("/static", "./static"))
            .route("/", web::get().to(index))
            .route("/login/student", web::get().to(student_login_page))
            .route("/login/lecturer", web::get().to(lecturer_login_page))
            .route("/login/admin", web::get().to(admin_login_page))
            .route("/student/home", web::get().to(student_home))
            .route("/lecturer/home", web::get().to(lecturer_home))
            .route("/admin/home", web::get().to(admin_home))
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}