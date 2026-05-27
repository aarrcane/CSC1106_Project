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

    let mut ctx = Context::new();
    ctx.insert("email_value", "");
    ctx.insert("error_message", "");
    ctx.insert("has_error", &false);
    let rendered = tmpl.render("index.html", &ctx).unwrap();

    HttpResponse::Ok().content_type("text/html").body(rendered)
}

//TODO: Add session handling
async fn student_dashboard(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let _user = match auth::require_role(&session, UserRole::Student) {
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

async fn student_announcement(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
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

    let rendered = match tmpl.render("student/announcement.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

async fn student_quiz(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match auth::require_role(&session, UserRole::Student) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    insert_student_base(&mut ctx, &user.display_name, "2501129");
    ctx.insert("active_page", "quizzes");

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

    // TODO: replace with DB query: SELECT * FROM quizzes JOIN enrollments WHERE student_id = ?
    let quizzes: Vec<QuizContext> = vec![
        QuizContext {
            id: 1,
            title: "Quiz 1 – HTML & CSS Basics".into(),
            course_code: "CSC1106".into(),
            course_name: "Web Programming".into(),
            due_date: "10 Apr 2026".into(),
            duration_mins: 20,
            status: "completed".into(),
            score: Some("18 / 20".into()),
            total_marks: 20,
            attempt_allowed: 1,
            attempts_used: 1,
            urgent: false,
        },
        QuizContext {
            id: 2,
            title: "Quiz 2 – JavaScript Fundamentals".into(),
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
        },
        QuizContext {
            id: 3,
            title: "Quiz 3 – Process Scheduling".into(),
            course_code: "CSC1107".into(),
            course_name: "Operating Systems".into(),
            due_date: "30 May 2026".into(),
            duration_mins: 30,
            status: "upcoming".into(),
            score: None,
            total_marks: 30,
            attempt_allowed: 1,
            attempts_used: 0,
            urgent: false,
        },
        QuizContext {
            id: 4,
            title: "Quiz 1 – Relational Model".into(),
            course_code: "INF2003".into(),
            course_name: "Database Systems".into(),
            due_date: "5 Apr 2026".into(),
            duration_mins: 20,
            status: "completed".into(),
            score: Some("19 / 20".into()),
            total_marks: 20,
            attempt_allowed: 1,
            attempts_used: 1,
            urgent: false,
        },
        QuizContext {
            id: 5,
            title: "Quiz 2 – Memory Management".into(),
            course_code: "CSC1107".into(),
            course_name: "Operating Systems".into(),
            due_date: "2 Apr 2026".into(),
            duration_mins: 20,
            status: "missed".into(),
            score: None,
            total_marks: 20,
            attempt_allowed: 1,
            attempts_used: 0,
            urgent: false,
        },
    ];
    // Pre-compute stat-card counts (Tera doesn't support "in" or | list filters)
    let quiz_open_count = quizzes.iter()
        .filter(|q| q.status == "open")
        .count();
    let quiz_upcoming_count = quizzes.iter()
        .filter(|q| q.status == "upcoming" || q.status == "open")
        .count();
    let quiz_completed_count = quizzes.iter()
        .filter(|q| q.status == "completed")
        .count();
    let quiz_missed_count = quizzes.iter()
        .filter(|q| q.status == "missed")
        .count();

    ctx.insert("quizzes",              &quizzes);
    ctx.insert("quiz_open_count",      &quiz_open_count);
    ctx.insert("quiz_upcoming_count",  &quiz_upcoming_count);
    ctx.insert("quiz_completed_count", &quiz_completed_count);
    ctx.insert("quiz_missed_count",    &quiz_missed_count);

    let rendered = match tmpl.render("student/quiz.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

async fn student_attendance(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match auth::require_role(&session, UserRole::Student) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    insert_student_base(&mut ctx, &user.display_name, "2501129");
    ctx.insert("active_page", "attendance");

    // TODO: replace with DB query for enrolled courses (filter dropdown)
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

    // TODO: replace with DB query: SELECT sessions, attendance_records WHERE student_id = ?
    let attendance_courses: Vec<AttendanceCourseContext> = vec![
        AttendanceCourseContext {
            code: "CSC1106".into(),
            name: "Web Programming".into(),
            pct: 90,
            attended: 9,
            total: 10,
            sessions: vec![
                AttendanceSessionContext { date: "5 Mar 2026".into(),  topic: "Introduction to HTML".into(),     status: "present".into() },
                AttendanceSessionContext { date: "12 Mar 2026".into(), topic: "CSS Layouts & Flexbox".into(),    status: "present".into() },
                AttendanceSessionContext { date: "19 Mar 2026".into(), topic: "JavaScript Basics".into(),        status: "present".into() },
                AttendanceSessionContext { date: "26 Mar 2026".into(), topic: "DOM Manipulation".into(),         status: "absent".into()  },
                AttendanceSessionContext { date: "2 Apr 2026".into(),  topic: "Fetch API & AJAX".into(),         status: "present".into() },
                AttendanceSessionContext { date: "9 Apr 2026".into(),  topic: "Forms & Validation".into(),       status: "present".into() },
                AttendanceSessionContext { date: "16 Apr 2026".into(), topic: "Responsive Design".into(),        status: "present".into() },
                AttendanceSessionContext { date: "23 Apr 2026".into(), topic: "Frameworks Overview".into(),      status: "present".into() },
                AttendanceSessionContext { date: "7 May 2026".into(),  topic: "Backend Integration".into(),      status: "present".into() },
                AttendanceSessionContext { date: "14 May 2026".into(), topic: "Project Workshop".into(),         status: "present".into() },
            ],
        },
        AttendanceCourseContext {
            code: "CSC1107".into(),
            name: "Operating Systems".into(),
            pct: 85,
            attended: 11,
            total: 13,
            sessions: vec![
                AttendanceSessionContext { date: "4 Mar 2026".into(),  topic: "OS Overview".into(),              status: "present".into() },
                AttendanceSessionContext { date: "11 Mar 2026".into(), topic: "Process Management".into(),       status: "present".into() },
                AttendanceSessionContext { date: "18 Mar 2026".into(), topic: "CPU Scheduling".into(),           status: "late".into()    },
                AttendanceSessionContext { date: "25 Mar 2026".into(), topic: "Deadlocks".into(),                status: "present".into() },
                AttendanceSessionContext { date: "1 Apr 2026".into(),  topic: "Memory Management".into(),        status: "absent".into()  },
                AttendanceSessionContext { date: "8 Apr 2026".into(),  topic: "Virtual Memory".into(),           status: "present".into() },
                AttendanceSessionContext { date: "15 Apr 2026".into(), topic: "File Systems".into(),             status: "present".into() },
                AttendanceSessionContext { date: "22 Apr 2026".into(), topic: "I/O Systems".into(),              status: "absent".into()  },
                AttendanceSessionContext { date: "6 May 2026".into(),  topic: "Security Basics".into(),          status: "present".into() },
                AttendanceSessionContext { date: "13 May 2026".into(), topic: "Virtualisation".into(),           status: "present".into() },
                AttendanceSessionContext { date: "20 May 2026".into(), topic: "Cloud OS Concepts".into(),        status: "present".into() },
                AttendanceSessionContext { date: "22 May 2026".into(), topic: "Revision Session".into(),         status: "present".into() },
                AttendanceSessionContext { date: "27 May 2026".into(), topic: "Exam Prep Q&A".into(),            status: "present".into() },
            ],
        },
        AttendanceCourseContext {
            code: "INF2003".into(),
            name: "Database Systems".into(),
            pct: 95,
            attended: 10,
            total: 11, // Removed 1 absent -> actually keep it honest
            sessions: vec![
                AttendanceSessionContext { date: "6 Mar 2026".into(),  topic: "Relational Model".into(),         status: "present".into() },
                AttendanceSessionContext { date: "13 Mar 2026".into(), topic: "SQL Basics".into(),               status: "present".into() },
                AttendanceSessionContext { date: "20 Mar 2026".into(), topic: "Advanced SQL".into(),             status: "present".into() },
                AttendanceSessionContext { date: "27 Mar 2026".into(), topic: "Normalisation".into(),            status: "present".into() },
                AttendanceSessionContext { date: "3 Apr 2026".into(),  topic: "ER Diagrams".into(),              status: "present".into() },
                AttendanceSessionContext { date: "10 Apr 2026".into(), topic: "Transactions & ACID".into(),      status: "present".into() },
                AttendanceSessionContext { date: "17 Apr 2026".into(), topic: "Indexing & Performance".into(),   status: "excused".into() },
                AttendanceSessionContext { date: "24 Apr 2026".into(), topic: "NoSQL Overview".into(),           status: "present".into() },
                AttendanceSessionContext { date: "8 May 2026".into(),  topic: "Database Security".into(),        status: "present".into() },
                AttendanceSessionContext { date: "15 May 2026".into(), topic: "Lab: Schema Design".into(),       status: "present".into() },
                AttendanceSessionContext { date: "22 May 2026".into(), topic: "Project Consultation".into(),     status: "present".into() },
            ],
        },
    ];

    // Derive overall stats from the course data
    let total_sessions: i32     = attendance_courses.iter().map(|c| c.total).sum();
    let attended_sessions: i32  = attendance_courses.iter().map(|c| c.attended).sum();
    let absent_sessions: i32    = attendance_courses.iter().flat_map(|c| &c.sessions)
        .filter(|s| s.status == "absent").count() as i32;
    let overall_pct: i32 = if total_sessions > 0 {
        (attended_sessions * 100) / total_sessions
    } else {
        0
    };

    ctx.insert("attendance_courses", &attendance_courses);
    ctx.insert("total_sessions",    &total_sessions);
    ctx.insert("attended_sessions", &attended_sessions);
    ctx.insert("absent_sessions",   &absent_sessions);
    ctx.insert("overall_pct",       &overall_pct);

    let rendered = match tmpl.render("student/attendance.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

async fn student_forum(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match auth::require_role(&session, UserRole::Student) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    insert_student_base(&mut ctx, &user.display_name, "2501129");
    ctx.insert("active_page", "forum");

    // TODO: replace with DB query for enrolled courses (filter dropdown)
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

    // TODO: replace with DB query: SELECT * FROM forum_threads WHERE course_id IN (enrolled) ORDER BY last_reply_at DESC
    let threads: Vec<ThreadContext> = vec![
        ThreadContext {
            id: 1,
            title: "How do I centre a div vertically in CSS?".into(),
            course_code: "CSC1106".into(),
            course_name: "Web Programming".into(),
            author: "Lee Zhi Yu".into(),
            author_initials: "LZ".into(),
            created_at: "20 May 2026".into(),
            last_reply_at: "24 May 2026".into(),
            reply_count: 5,
            view_count: 42,
            is_pinned: false,
            is_answered: true,
            is_mine: true,
            tags: vec!["css".into(), "question".into()],
            preview: "I've been trying to vertically centre a div inside a full-height container but flexbox doesn't seem to work as expected. Any tips?".into(),
        },
        ThreadContext {
            id: 2,
            title: "[PINNED] Assignment 2 – Clarifications & FAQ".into(),
            course_code: "CSC1106".into(),
            course_name: "Web Programming".into(),
            author: "Dr. Tan Wei Ming".into(),
            author_initials: "TW".into(),
            created_at: "24 May 2026".into(),
            last_reply_at: "25 May 2026".into(),
            reply_count: 12,
            view_count: 198,
            is_pinned: true,
            is_answered: false,
            is_mine: false,
            tags: vec!["announcement".into(), "assignment".into()],
            preview: "This thread collects all common questions about Assignment 2. Please read before posting a new question. Submission deadline: 28 May 2026.".into(),
        },
        ThreadContext {
            id: 3,
            title: "Confused about the difference between paging and segmentation".into(),
            course_code: "CSC1107".into(),
            course_name: "Operating Systems".into(),
            author: "Aisha Rahman".into(),
            author_initials: "AR".into(),
            created_at: "22 May 2026".into(),
            last_reply_at: "23 May 2026".into(),
            reply_count: 3,
            view_count: 27,
            is_pinned: false,
            is_answered: true,
            is_mine: false,
            tags: vec!["memory".into(), "question".into()],
            preview: "The lecture slides mention both paging and segmentation for memory management. Can someone explain the key difference with a concrete example?".into(),
        },
        ThreadContext {
            id: 4,
            title: "ER diagram – should relationships have attributes?".into(),
            course_code: "INF2003".into(),
            course_name: "Database Systems".into(),
            author: "Raj Kumar".into(),
            author_initials: "RK".into(),
            created_at: "21 May 2026".into(),
            last_reply_at: "21 May 2026".into(),
            reply_count: 1,
            view_count: 15,
            is_pinned: false,
            is_answered: false,
            is_mine: false,
            tags: vec!["er-diagram".into()],
            preview: "I know entities have attributes, but for my project I need to store extra info about a relationship. Is that allowed in standard ER notation?".into(),
        },
        ThreadContext {
            id: 5,
            title: "Actix-web vs Axum – which is easier for beginners?".into(),
            course_code: "CSC1106".into(),
            course_name: "Web Programming".into(),
            author: "Mei Ling Tan".into(),
            author_initials: "ML".into(),
            created_at: "18 May 2026".into(),
            last_reply_at: "20 May 2026".into(),
            reply_count: 8,
            view_count: 63,
            is_pinned: false,
            is_answered: true,
            is_mine: false,
            tags: vec!["rust".into(), "backend".into()],
            preview: "We covered Actix in class but I saw Axum mentioned online. For someone just starting out with Rust web dev, which framework is more beginner-friendly?".into(),
        },
        ThreadContext {
            id: 6,
            title: "Lab 3 – getting a foreign key constraint error on INSERT".into(),
            course_code: "INF2003".into(),
            course_name: "Database Systems".into(),
            author: "Lee Zhi Yu".into(),
            author_initials: "LZ".into(),
            created_at: "15 May 2026".into(),
            last_reply_at: "16 May 2026".into(),
            reply_count: 2,
            view_count: 19,
            is_pinned: false,
            is_answered: true,
            is_mine: true,
            tags: vec!["sql".into(), "lab".into()],
            preview: "Running the INSERT in Lab 3 throws: ERROR 1452 – Cannot add or update a child row: a foreign key constraint fails. The parent row definitely exists, so I'm not sure what's wrong.".into(),
        },
    ];
    ctx.insert("threads", &threads);

    let rendered = match tmpl.render("student/discussionforum.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

async fn lecturer_dashboard(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    ctx.insert("display_name", &user.display_name);
    ctx.insert("student_name", &user.display_name);
    ctx.insert("student_id", "");
    ctx.insert("notifications", &Vec::<NotificationContext>::new());
    ctx.insert("active_page", "dashboard");
    ctx.insert("is_lecturer", &true);
    ctx.insert("assigned_courses_count", &4);
    ctx.insert("student_count", &128);
    ctx.insert("pending_grades_count", &17);
    ctx.insert("forum_questions_count", &9);

    #[derive(Serialize)]
    struct LecturerCourse {
        code: String,
        name: String,
        term: String,
        students: i32,
        status: String,
    }
    let assigned_courses = vec![
        LecturerCourse { code: "CSC2101".into(), name: "Web Development II".into(), term: "2025/26 Trimester 3".into(), students: 42, status: "Ongoing".into() },
        LecturerCourse { code: "CSC2203".into(), name: "Software Engineering".into(), term: "2025/26 Trimester 3".into(), students: 38, status: "Ongoing".into() },
        LecturerCourse { code: "CSC2304".into(), name: "Mobile App Development".into(), term: "2025/26 Trimester 3".into(), students: 31, status: "Ongoing".into() },
        LecturerCourse { code: "CSC2405".into(), name: "Cloud Fundamentals".into(), term: "2025/26 Trimester 3".into(), students: 17, status: "Preparing".into() },
    ];
    ctx.insert("assigned_courses", &assigned_courses);

    #[derive(Serialize)]
    struct PendingSubmission {
        title: String,
        course: String,
        submitted_by: String,
        due: String,
        pending_count: i32,
    }
    let pending_submissions = vec![
        PendingSubmission { title: "Assignment 2".into(), course: "CSC2101".into(), submitted_by: "14 students".into(), due: "28 May 2026".into(), pending_count: 14 },
        PendingSubmission { title: "Lab Report 3".into(), course: "CSC2203".into(), submitted_by: "9 students".into(), due: "30 May 2026".into(), pending_count: 9 },
    ];
    ctx.insert("pending_submissions", &pending_submissions);

    #[derive(Serialize)]
    struct ForumQuestion {
        title: String,
        course: String,
        author: String,
        when: String,
    }
    let forum_questions = vec![
        ForumQuestion { title: "Can we use CSS Grid for the layout?".into(), course: "CSC2101".into(), author: "Aisha".into(), when: "2h ago".into() },
        ForumQuestion { title: "Is the quiz open-book?".into(), course: "CSC2203".into(), author: "Daniel".into(), when: "5h ago".into() },
        ForumQuestion { title: "Deployment issue on Windows".into(), course: "CSC2304".into(), author: "Wei Ming".into(), when: "1d ago".into() },
    ];
    ctx.insert("forum_questions", &forum_questions);

    #[derive(Serialize)]
    struct UpcomingEvent {
        title: String,
        course: String,
        when: String,
    }
    let upcoming_events = vec![
        UpcomingEvent { title: "Lecture 7: Routing".into(), course: "CSC2101".into(), when: "Tomorrow 9:00 AM".into() },
        UpcomingEvent { title: "Assignment 2 due".into(), course: "CSC2101".into(), when: "28 May 2026".into() },
        UpcomingEvent { title: "Lab session".into(), course: "CSC2203".into(), when: "29 May 2026".into() },
    ];
    ctx.insert("upcoming_events", &upcoming_events);

    let rendered = match tmpl.render("lecturer/dashboard.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

async fn lecturer_courses_page(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    ctx.insert("display_name", &user.display_name);
    ctx.insert("student_name", &user.display_name);
    ctx.insert("student_id", "");
    ctx.insert("notifications", &Vec::<NotificationContext>::new());
    ctx.insert("active_page", "courses");
    ctx.insert("is_lecturer", &true);

    let rendered = match tmpl.render("lecturer/course.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

async fn lecturer_assignments_page(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    ctx.insert("display_name", &user.display_name);
    ctx.insert("student_name", &user.display_name);
    ctx.insert("student_id", "");
    ctx.insert("notifications", &Vec::<NotificationContext>::new());
    ctx.insert("active_page", "assignments");
    ctx.insert("is_lecturer", &true);

    let rendered = match tmpl.render("lecturer/assignments.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

async fn lecturer_quizzes_page(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    ctx.insert("display_name", &user.display_name);
    ctx.insert("student_name", &user.display_name);
    ctx.insert("student_id", "");
    ctx.insert("notifications", &Vec::<NotificationContext>::new());
    ctx.insert("active_page", "quizzes");
    ctx.insert("is_lecturer", &true);

    let rendered = match tmpl.render("lecturer/quizzes.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

async fn lecturer_grades_page(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    ctx.insert("display_name", &user.display_name);
    ctx.insert("student_name", &user.display_name);
    ctx.insert("student_id", "");
    ctx.insert("notifications", &Vec::<NotificationContext>::new());
    ctx.insert("active_page", "grades");
    ctx.insert("is_lecturer", &true);

    let rendered = match tmpl.render("lecturer/grades.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

async fn lecturer_attendance_page(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    ctx.insert("display_name", &user.display_name);
    ctx.insert("student_name", &user.display_name);
    ctx.insert("student_id", "");
    ctx.insert("notifications", &Vec::<NotificationContext>::new());
    ctx.insert("active_page", "attendance");
    ctx.insert("is_lecturer", &true);

    let rendered = match tmpl.render("lecturer/attendance.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

async fn lecturer_forum_page(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    ctx.insert("display_name", &user.display_name);
    ctx.insert("student_name", &user.display_name);
    ctx.insert("student_id", "");
    ctx.insert("notifications", &Vec::<NotificationContext>::new());
    ctx.insert("active_page", "forum");
    ctx.insert("is_lecturer", &true);

    let rendered = match tmpl.render("lecturer/forum.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

async fn lecturer_profile_page(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    ctx.insert("display_name", &user.display_name);
    ctx.insert("student_name", &user.display_name);
    ctx.insert("student_id", "");
    ctx.insert("notifications", &Vec::<NotificationContext>::new());
    ctx.insert("active_page", "profile");
    ctx.insert("is_lecturer", &true);

    let rendered = match tmpl.render("lecturer/profile.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

async fn lecturer_settings_page(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    ctx.insert("display_name", &user.display_name);
    ctx.insert("student_name", &user.display_name);
    ctx.insert("student_id", "");
    ctx.insert("notifications", &Vec::<NotificationContext>::new());
    ctx.insert("active_page", "settings");
    ctx.insert("is_lecturer", &true);

    let rendered = match tmpl.render("lecturer/settings.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

async fn admin_dashboard(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match auth::require_role(&session, UserRole::Admin) {
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
    let notifications: Vec<NotificationContext> = vec![];
    ctx.insert("notifications", &notifications);
    // Highlight active sidebar item
    ctx.insert("active_page", "dashboard");
    // Mark template as admin so shared partials can adapt
    ctx.insert("is_admin", &true);
    // Admin dashboard counts (hardcoded for now). Replace with DB queries in future.
    ctx.insert("students_count", &1240);
    ctx.insert("lecturers_count", &85);
    ctx.insert("courses_count", &42);
    ctx.insert("enrollments_count", &3120);
    ctx.insert("admins_count", &3);
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

async fn admin_users_page(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match auth::require_role(&session, UserRole::Admin) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    ctx.insert("display_name", &user.display_name);
    ctx.insert("student_name", &user.display_name);
    ctx.insert("student_id", "");
    ctx.insert("notifications", &Vec::<NotificationContext>::new());
    ctx.insert("active_page", "users");
    ctx.insert("is_admin", &true);

    let rendered = match tmpl.render("admin/user_management.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

async fn admin_courses_page(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match auth::require_role(&session, UserRole::Admin) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    ctx.insert("display_name", &user.display_name);
    ctx.insert("student_name", &user.display_name);
    ctx.insert("student_id", "");
    ctx.insert("notifications", &Vec::<NotificationContext>::new());
    ctx.insert("active_page", "courses");
    ctx.insert("is_admin", &true);

    let rendered = match tmpl.render("admin/course_administration.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

async fn admin_content_page(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match auth::require_role(&session, UserRole::Admin) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    ctx.insert("display_name", &user.display_name);
    ctx.insert("student_name", &user.display_name);
    ctx.insert("student_id", "");
    ctx.insert("notifications", &Vec::<NotificationContext>::new());
    ctx.insert("active_page", "content");
    ctx.insert("is_admin", &true);

    let rendered = match tmpl.render("admin/content_oversight.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

async fn admin_settings_page(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match auth::require_role(&session, UserRole::Admin) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    ctx.insert("display_name", &user.display_name);
    ctx.insert("student_name", &user.display_name);
    ctx.insert("student_id", "");
    ctx.insert("notifications", &Vec::<NotificationContext>::new());
    ctx.insert("active_page", "settings");
    ctx.insert("is_admin", &true);

    let rendered = match tmpl.render("admin/global_settings.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

async fn admin_audit_page(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match auth::require_role(&session, UserRole::Admin) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    ctx.insert("display_name", &user.display_name);
    ctx.insert("student_name", &user.display_name);
    ctx.insert("student_id", "");
    ctx.insert("notifications", &Vec::<NotificationContext>::new());
    ctx.insert("active_page", "audit");
    ctx.insert("is_admin", &true);

    let rendered = match tmpl.render("admin/security_audit.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
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
            .route("/login", web::get().to(index))
            .route("/login", web::post().to(auth::login_submit))
            .route("/logout", web::post().to(auth::logout))

            // Student Routes
            .route("/student/dashboard", web::get().to(student_dashboard))
            .route("/student/courses", web::get().to(student_courses))
            .route("/student/assignments", web::get().to(student_assignments))
            .route("/student/grades", web::get().to(student_grades))
            .route("/student/announcement", web::get().to(student_announcement))
            .route("/student/quizzes",      web::get().to(student_quiz))
            .route("/student/attendance",   web::get().to(student_attendance))
            .route("/student/forum",        web::get().to(student_forum))
            //.route("/student/home", web::get().to(student_home)) //to be removed

            // Lecturer Routes
            .route("/lecturer/dashboard", web::get().to(lecturer_dashboard))
            .route("/lecturer/courses", web::get().to(lecturer_courses_page))
            .route("/lecturer/assignments", web::get().to(lecturer_assignments_page))
            .route("/lecturer/quizzes", web::get().to(lecturer_quizzes_page))
            .route("/lecturer/grades", web::get().to(lecturer_grades_page))
            .route("/lecturer/attendance", web::get().to(lecturer_attendance_page))
            .route("/lecturer/forum", web::get().to(lecturer_forum_page))
            .route("/lecturer/profile", web::get().to(lecturer_profile_page))
            .route("/lecturer/settings", web::get().to(lecturer_settings_page))

            // Admin Routes
            .route("/admin/dashboard", web::get().to(admin_dashboard))
            .route("/admin/users", web::get().to(admin_users_page))
            .route("/admin/courses", web::get().to(admin_courses_page))
            .route("/admin/content", web::get().to(admin_content_page))
            .route("/admin/settings", web::get().to(admin_settings_page))
            .route("/admin/audit", web::get().to(admin_audit_page))

            // API Routes (JSON)
            .route("/api/students", web::get().to(get_students))
            .route("/api/students", web::post().to(create_student))
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
