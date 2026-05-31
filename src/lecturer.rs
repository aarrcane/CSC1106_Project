use actix_session::Session;
use actix_web::{web, HttpResponse, Responder};
use tera::{Context, Tera};
use sqlx::PgPool;
use serde::Serialize;

use crate::auth::UserRole;

pub async fn lecturer_dashboard(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    ctx.insert("display_name", &user.display_name);
    ctx.insert("student_name", &user.display_name);
    ctx.insert("student_id", "");
    ctx.insert("notifications", &Vec::<crate::NotificationContext>::new());
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

pub async fn lecturer_courses_page(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    ctx.insert("display_name", &user.display_name);
    ctx.insert("student_name", &user.display_name);
    ctx.insert("student_id", "");
    ctx.insert("notifications", &Vec::<crate::NotificationContext>::new());
    ctx.insert("active_page", "courses");
    ctx.insert("is_lecturer", &true);

    let rendered = match tmpl.render("lecturer/course.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

pub async fn lecturer_assignments_page(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    ctx.insert("display_name", &user.display_name);
    ctx.insert("student_name", &user.display_name);
    ctx.insert("student_id", "");
    ctx.insert("notifications", &Vec::<crate::NotificationContext>::new());
    ctx.insert("active_page", "assignments");
    ctx.insert("is_lecturer", &true);

    let rendered = match tmpl.render("lecturer/assignments.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

pub async fn lecturer_quizzes_page(
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    ctx.insert("display_name", &user.display_name);
    ctx.insert("student_name", &user.display_name);
    ctx.insert("student_id", "");
    ctx.insert("notifications", &Vec::<crate::NotificationContext>::new());
    ctx.insert("active_page", "quizzes");
    ctx.insert("is_lecturer", &true);

    let events = match sqlx::query_as::<_, crate::QuizMonitoringEventContext>(
        "SELECT
            id,
            quiz_id,
            student_display_name,
            event_type,
            severity,
            details,
            TO_CHAR(occurred_at AT TIME ZONE 'Asia/Singapore', 'YYYY-MM-DD HH24:MI:SS') AS occurred_at
         FROM quiz_monitoring_events
         ORDER BY occurred_at DESC
         LIMIT 50",
    )
    .fetch_all(db.get_ref())
    .await
    {
        Ok(events) => events,
        Err(error) => {
            ctx.insert(
                "monitoring_load_error",
                &format!("Could not load quiz monitoring events: {error}"),
            );
            Vec::new()
        }
    };
    ctx.insert("monitoring_events", &events);
    ctx.insert("monitoring_event_count", &events.len());

    let rendered = match tmpl.render("lecturer/quizzes.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

pub async fn lecturer_grades_page(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    ctx.insert("display_name", &user.display_name);
    ctx.insert("student_name", &user.display_name);
    ctx.insert("student_id", "");
    ctx.insert("notifications", &Vec::<crate::NotificationContext>::new());
    ctx.insert("active_page", "grades");
    ctx.insert("is_lecturer", &true);

    let rendered = match tmpl.render("lecturer/grades.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

pub async fn lecturer_attendance_page(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    ctx.insert("display_name", &user.display_name);
    ctx.insert("student_name", &user.display_name);
    ctx.insert("student_id", "");
    ctx.insert("notifications", &Vec::<crate::NotificationContext>::new());
    ctx.insert("active_page", "attendance");
    ctx.insert("is_lecturer", &true);

    let rendered = match tmpl.render("lecturer/attendance.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

pub async fn lecturer_forum_page(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    ctx.insert("display_name", &user.display_name);
    ctx.insert("student_name", &user.display_name);
    ctx.insert("student_id", "");
    ctx.insert("notifications", &Vec::<crate::NotificationContext>::new());
    ctx.insert("active_page", "forum");
    ctx.insert("is_lecturer", &true);

    let rendered = match tmpl.render("lecturer/forum.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

pub async fn lecturer_profile_page(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    ctx.insert("display_name", &user.display_name);
    ctx.insert("student_name", &user.display_name);
    ctx.insert("student_id", "");
    ctx.insert("notifications", &Vec::<crate::NotificationContext>::new());
    ctx.insert("active_page", "profile");
    ctx.insert("is_lecturer", &true);

    let rendered = match tmpl.render("lecturer/profile.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

pub async fn lecturer_settings_page(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    ctx.insert("display_name", &user.display_name);
    ctx.insert("student_name", &user.display_name);
    ctx.insert("student_id", "");
    ctx.insert("notifications", &Vec::<crate::NotificationContext>::new());
    ctx.insert("active_page", "settings");
    ctx.insert("is_lecturer", &true);

    let rendered = match tmpl.render("lecturer/settings.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}
