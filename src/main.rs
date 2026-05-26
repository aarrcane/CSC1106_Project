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

fn insert_student_base(ctx: &mut Context, display_name: &str, student_id: &str) {
    ctx.insert("student_name", display_name);
    ctx.insert("student_id", student_id);
    //TODO: Replace with real DB query for unread notifications
    let notifications: Vec<NotificationContext> = vec![];
    ctx.insert("notifications", &notifications);
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

// To be removed
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

async fn admin_login_page(tmpl: web::Data<Tera>) -> impl Responder {
    let mut ctx = Context::new();
    ctx.insert("role_name", "Admin");
    ctx.insert("username_label", "Email");
    ctx.insert("action_url", "/admin/home");
    let rendered = tmpl.render("login.html", &ctx).unwrap();

    HttpResponse::Ok().content_type("text/html").body(rendered)
}

//TODO: Add session handling
async fn student_dashboard(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match auth::require_role(&session, UserRole::Student) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();

    let notifications: Vec<NotificationContext> = vec![];
    ctx.insert("notifications", &notifications);

    // TODO: Replace with session-based user lookup
    ctx.insert("student_name", "Lee Zhi Yu");
    ctx.insert("student_id", "2501129");
    ctx.insert("current_trimester", "2025/26 Trimester 3");
    ctx.insert("current_date", "Monday, 25 May 2026");

    // TODO: Replace with DB query: SELECT COUNT(*) FROM enrollments(?) WHERE student_id = ?
    ctx.insert("enrolled_course_count", &3);
    ctx.insert("avg_grade", &78);
    ctx.insert("attendance_pct", &91);
    ctx.insert("upcoming_deadlines", &2);

    // Sidebar active page highlight
    ctx.insert("active_page", "dashboard");

    let courses: Vec<CourseContext> = vec![
         CourseContext {
            id: 1,
            code: "CSC1106".into(),
            name: "Web Programming".into(),
            trimester: "2025/26 Trimester 3".into(),
            image_url: "".into(),
            pinned: true,
            ongoing: true,
            progress: 65,
            lecturer: "Dr. Tan Wei Ming".into(),
            attendance_pct: 90,
        },
        CourseContext {
            id: 2,
            code: "CSC1107".into(),
            name: "Operating Systems".into(),
            trimester: "2025/26 Trimester 3".into(),
            image_url: "".into(),
            pinned: false,
            ongoing: true,
            progress: 50,
            lecturer: "Prof. Lim Ah Kow".into(),
            attendance_pct: 85,
        },
        CourseContext {
            id: 3,
            code: "INF2003".into(),
            name: "Database Systems".into(),
            trimester: "2025/26 Trimester 3".into(),
            image_url: "".into(),
            pinned: false,
            ongoing: true,
            progress: 72,
            lecturer: "Dr. Ng Siew Lin".into(),
            attendance_pct: 95,
        },
    ];
    ctx.insert("courses", &courses);
    ctx.insert("trimesters", &vec!["2025/26 Trimester 3"]);

    let announcements: Vec<AnnouncementContext> = vec![
        AnnouncementContext {
            title: "Assignment 2 brief released".into(),
            course: "CSC1106 – Web Programming".into(),
            date: "24 May 2026".into(),
        },
    ];
    ctx.insert("announcements", &announcements);

    let due_dates: Vec<DueDateContext> = vec![
                DueDateContext {
            title: "Assignment 2 Submission".into(),
            course: "CSC1106".into(),
            item_type: "assignment".into(),
            due_date: "28 May".into(),
            urgent: true,
        },
        DueDateContext {
            title: "Quiz 3".into(),
            course: "CSC1107".into(),
            item_type: "quiz".into(),
            due_date: "30 May".into(),
            urgent: false,
        },
    ];
    ctx.insert("due_dates", &due_dates);

    let rendered = match tmpl.render("student/dashboard.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

async fn student_courses(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match auth::require_role(&session, UserRole::Student) {
        Ok(user) => user,
        Err(response) => return response,
    };
    
    let mut ctx = Context::new();
    insert_student_base(&mut ctx, &user.display_name, "2501129");
    ctx.insert("active_page", "courses");
    ctx.insert("current_trimester", "2025/26 Trimester 3");

    // TODO: replace with DB query: SELECT * FROM courses JOIN enrollments WHERE student_id = ?
    let courses: Vec<CourseContext> = vec![
        CourseContext {
            id: 1,
            code: "CSC1106".into(),
            name: "Web Programming".into(),
            trimester: "2025/26 Trimester 3".into(),
            image_url: "".into(),
            pinned: true,
            ongoing: true,
            progress: 65,
            lecturer: "Dr. Tan Wei Ming".into(),
            attendance_pct: 90,
        },
        CourseContext {
            id: 2,
            code: "CSC1107".into(),
            name: "Operating Systems".into(),
            trimester: "2025/26 Trimester 3".into(),
            image_url: "".into(),
            pinned: false,
            ongoing: true,
            progress: 50,
            lecturer: "Prof. Lim Ah Kow".into(),
            attendance_pct: 85,
        },
        CourseContext {
            id: 3,
            code: "INF2003".into(),
            name: "Database Systems".into(),
            trimester: "2025/26 Trimester 3".into(),
            image_url: "".into(),
            pinned: false,
            ongoing: true,
            progress: 72,
            lecturer: "Dr. Ng Siew Lin".into(),
            attendance_pct: 95,
        },
    ];
    ctx.insert("courses", &courses);
    ctx.insert("trimesters", &vec!["2025/26 Trimester 3"]);

    let rendered = match tmpl.render("student/courses.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
    
}

async fn student_assignments(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match auth::require_role(&session, UserRole::Student) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    insert_student_base(&mut ctx, &user.display_name, "2501129");
    ctx.insert("active_page", "assignments");

    // TODO: replace with DB query for enrolled courses (used by filter dropdown)
    let courses: Vec<CourseContext> = vec![
        CourseContext {
            id: 1, code: "CSC1106".into(), name: "Web Programming".into(),
            trimester: "2025/26 Trimester 3".into(), image_url: "".into(),
            pinned: false, ongoing: true, progress: 65,
            lecturer: "Dr. Tan Wei Ming".into(), attendance_pct: 90,
        },
        CourseContext {
            id: 2, code: "CSC1107".into(), name: "Operating Systems".into(),
            trimester: "2025/26 Trimester 3".into(), image_url: "".into(),
            pinned: false, ongoing: true, progress: 50,
            lecturer: "Prof. Lim Ah Kow".into(), attendance_pct: 85,
        },
        CourseContext {
            id: 3, code: "INF2003".into(), name: "Database Systems".into(),
            trimester: "2025/26 Trimester 3".into(), image_url: "".into(),
            pinned: false, ongoing: true, progress: 72,
            lecturer: "Dr. Ng Siew Lin".into(), attendance_pct: 95,
        },
    ];
    ctx.insert("courses", &courses);

    // TODO: replace with DB query: SELECT * FROM assignments/quizzes WHERE student_id = ?
    let assignments: Vec<AssignmentContext> = vec![
        AssignmentContext {
            id: 1,
            title: "Assignment 1".into(),
            course_code: "CSC1106".into(),
            course_name: "Web Programming".into(),
            item_type: "assignment".into(),
            due_date: "15 May 2026".into(),
            status: "graded".into(),
            score: Some("82 / 100".into()),
            urgent: false,
        },
        AssignmentContext {
            id: 2,
            title: "Assignment 2".into(),
            course_code: "CSC1106".into(),
            course_name: "Web Programming".into(),
            item_type: "assignment".into(),
            due_date: "28 May 2026".into(),
            status: "pending".into(),
            score: None,
            urgent: true,
        },
        AssignmentContext {
            id: 3,
            title: "Quiz 3".into(),
            course_code: "CSC1107".into(),
            course_name: "Operating Systems".into(),
            item_type: "quiz".into(),
            due_date: "30 May 2026".into(),
            status: "pending".into(),
            score: None,
            urgent: false,
        },
        AssignmentContext {
            id: 4,
            title: "Lab Report 2".into(),
            course_code: "INF2003".into(),
            course_name: "Database Systems".into(),
            item_type: "assignment".into(),
            due_date: "10 May 2026".into(),
            status: "submitted".into(),
            score: None,
            urgent: false,
        },
    ];
    ctx.insert("assignments", &assignments);

    let rendered = match tmpl.render("student/assignments.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

async fn student_grades(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match auth::require_role(&session, UserRole::Student) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    insert_student_base(&mut ctx, &user.display_name, "2501129");
    ctx.insert("active_page", "grades");

    // TODO: replace with DB queries for actual grade data
    let course_grades: Vec<CourseGradeContext> = vec![
        CourseGradeContext {
            code: "CSC1106".into(),
            name: "Web Programming".into(),
            overall: 82.0,
            grade_letter: "A-".into(),
            items: vec![
                GradeItemContext { title: "Assignment 1".into(), item_type: "assignment".into(), score: 82.0, max_score: 100.0, weight: 20.0 },
                GradeItemContext { title: "Quiz 1".into(),       item_type: "quiz".into(),       score: 18.0, max_score: 20.0,  weight: 10.0 },
                GradeItemContext { title: "Midterm Exam".into(), item_type: "exam".into(),       score: 38.0, max_score: 50.0,  weight: 30.0 },
            ],
        },
        CourseGradeContext {
            code: "CSC1107".into(),
            name: "Operating Systems".into(),
            overall: 74.0,
            grade_letter: "B".into(),
            items: vec![
                GradeItemContext { title: "Assignment 1".into(), item_type: "assignment".into(), score: 75.0, max_score: 100.0, weight: 20.0 },
                GradeItemContext { title: "Quiz 2".into(),       item_type: "quiz".into(),       score: 14.0, max_score: 20.0,  weight: 10.0 },
                GradeItemContext { title: "Midterm Exam".into(), item_type: "exam".into(),       score: 35.0, max_score: 50.0,  weight: 30.0 },
            ],
        },
        CourseGradeContext {
            code: "INF2003".into(),
            name: "Database Systems".into(),
            overall: 89.0,
            grade_letter: "A".into(),
            items: vec![
                GradeItemContext { title: "Lab Report 1".into(), item_type: "assignment".into(), score: 90.0, max_score: 100.0, weight: 20.0 },
                GradeItemContext { title: "Quiz 1".into(),        item_type: "quiz".into(),      score: 19.0, max_score: 20.0,  weight: 10.0 },
                GradeItemContext { title: "Midterm Exam".into(),  item_type: "exam".into(),      score: 44.0, max_score: 50.0,  weight: 30.0 },
            ],
        },
    ];

    // Derived summary stats
    let overall_avg = if course_grades.is_empty() {
        0
    } else {
        (course_grades.iter().map(|c| c.overall).sum::<f32>() / course_grades.len() as f32) as i32
    };
    let highest_grade = course_grades.iter()
        .map(|c| c.overall as i32)
        .max()
        .unwrap_or(0);
    let at_risk_count = course_grades.iter()
        .filter(|c| c.overall < 60.0)
        .count();

    ctx.insert("course_grades", &course_grades);
    ctx.insert("overall_avg", &overall_avg);
    ctx.insert("highest_grade", &highest_grade);
    ctx.insert("at_risk_count", &at_risk_count);

    let rendered = match tmpl.render("student/grades.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

async fn student_annoucement(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match auth::require_role(&session, UserRole::Student) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    insert_student_base(&mut ctx, &user.display_name, "2501129");
    ctx.insert("active_page", "announcements");

    // TODO: replace with DB query for enrolled courses (used by filter dropdown)
    let courses: Vec<CourseContext> = vec![
        CourseContext {
            id: 1, code: "CSC1106".into(), name: "Web Programming".into(),
            trimester: "2025/26 Trimester 3".into(), image_url: "".into(),
            pinned: false, ongoing: true, progress: 65,
            lecturer: "Dr. Tan Wei Ming".into(), attendance_pct: 90,
        },
        CourseContext {
            id: 2, code: "CSC1107".into(), name: "Operating Systems".into(),
            trimester: "2025/26 Trimester 3".into(), image_url: "".into(),
            pinned: false, ongoing: true, progress: 50,
            lecturer: "Prof. Lim Ah Kow".into(), attendance_pct: 85,
        },
        CourseContext {
            id: 3, code: "INF2003".into(), name: "Database Systems".into(),
            trimester: "2025/26 Trimester 3".into(), image_url: "".into(),
            pinned: false, ongoing: true, progress: 72,
            lecturer: "Dr. Ng Siew Lin".into(), attendance_pct: 95,
        },
    ];
    ctx.insert("courses", &courses);

    // TODO: replace with DB query: SELECT * FROM announcements WHERE course_id IN (enrolled) ORDER BY date DESC
    let announcements: Vec<AnnouncementFullContext> = vec![
        AnnouncementFullContext {
            id: 1,
            title: "Assignment 2 brief released".into(),
            course: "Web Programming".into(),
            course_code: "CSC1106".into(),
            date: "24 May 2026".into(),
            content: "The brief for Assignment 2 has been uploaded to the course portal. Please review the requirements and submit by 28 May.".into(),
            is_new: true,
        },
        AnnouncementFullContext {
            id: 2,
            title: "Midterm rescheduled to Week 8".into(),
            course: "Operating Systems".into(),
            course_code: "CSC1107".into(),
            date: "22 May 2026".into(),
            content: "Due to the public holiday, the midterm exam has been moved to Week 8. New date: 5 June 2026 at 10am.".into(),
            is_new: true,
        },
        AnnouncementFullContext {
            id: 3,
            title: "Lab session cancelled this Friday".into(),
            course: "Database Systems".into(),
            course_code: "INF2003".into(),
            date: "20 May 2026".into(),
            content: "The lab session scheduled for Friday 23 May is cancelled. A replacement session will be arranged.".into(),
            is_new: false,
        },
    ];
    ctx.insert("announcements", &announcements);

    let rendered = match tmpl.render("student/annoucement.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
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

            //Static Files (CSS, JS, images)
            .service(Files::new("/static", "./static"))

            // Public Routes
            .route("/", web::get().to(index))
            .route("/login/{role}", web::get().to(auth::login_page))
            .route("/login/{role}", web::post().to(auth::login_submit))
            .route("/logout", web::post().to(auth::logout))

            // Student Routes
            .route("/student/dashboard", web::get().to(student_dashboard))
            .route("/student/courses", web::get().to(student_courses))
            .route("/student/assignments", web::get().to(student_assignments))
            .route("/student/grades", web::get().to(student_grades))
            .route("/student/annoucement", web::get().to(student_annoucement))
            .route("/student/home", web::get().to(student_home)) //to be removed

            // Lecturer Routes
            .route("/lecturer/home", web::get().to(lecturer_home))

            // Admin Routes
            .route("/admin/home", web::get().to(admin_home))
            .route("/admin/login", web::get().to(admin_login_page))

            // API Routes (JSON)
            .route("/api/students", web::get().to(get_students))
            .route("/api/students", web::post().to(create_student))
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
