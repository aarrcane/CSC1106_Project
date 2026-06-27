use actix_session::Session;
use actix_web::{HttpResponse, Responder, web};
use sqlx::PgPool;
use tera::{Context, Tera};

use crate::admin::{log_audit_event, AuditActor};
use crate::auth::UserRole;

#[derive(serde::Serialize, sqlx::FromRow)]
struct StudentProfileDetails {
    display_name: String,
    email: String,
    role: String,
    is_active: bool,
    created_at: String,
    student_id: i32,
    age: Option<i32>,
    programme: Option<String>,
    year_of_study: Option<i32>,
    enrolled_courses: i64,
}

#[derive(serde::Serialize, sqlx::FromRow)]
struct UserPreferenceDetails {
    email_notifications: bool,
    course_notifications: bool,
    forum_notifications: bool,
    grade_notifications: bool,
    theme_mode: String,
}

#[derive(serde::Deserialize)]
pub struct UserPreferencesForm {
    email_notifications: Option<String>,
    course_notifications: Option<String>,
    forum_notifications: Option<String>,
    grade_notifications: Option<String>,
    theme_mode: String,
}

async fn ensure_user_preferences_table(db: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS user_preferences (
            user_id INT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
            email_notifications BOOLEAN NOT NULL DEFAULT TRUE,
            course_notifications BOOLEAN NOT NULL DEFAULT TRUE,
            forum_notifications BOOLEAN NOT NULL DEFAULT TRUE,
            grade_notifications BOOLEAN NOT NULL DEFAULT TRUE,
            theme_mode VARCHAR(20) NOT NULL DEFAULT 'light' CHECK (theme_mode IN ('light', 'dark')),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )",
    )
    .execute(db)
    .await
    .map(|_| ())
}

async fn load_user_preferences(
    db: &PgPool,
    user_id: i32,
) -> Result<UserPreferenceDetails, sqlx::Error> {
    ensure_user_preferences_table(db).await?;
    sqlx::query(
        "INSERT INTO user_preferences (user_id)
         VALUES ($1)
         ON CONFLICT (user_id) DO NOTHING",
    )
    .bind(user_id)
    .execute(db)
    .await?;

    sqlx::query_as::<_, UserPreferenceDetails>(
        "SELECT email_notifications, course_notifications, forum_notifications,
                grade_notifications, theme_mode
         FROM user_preferences
         WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_one(db)
    .await
}

pub async fn student_dashboard(
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Student) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();

    let notifications: Vec<crate::NotificationContext> = vec![];
    ctx.insert("notifications", &notifications);

    // Use logged-in user's display name
    ctx.insert("student_name", &user.display_name);

    // Attempt to fetch student record to show student id (if exists)
    let student_id_opt =
        sqlx::query_scalar::<_, i32>("SELECT id FROM students WHERE user_id = $1 LIMIT 1")
            .bind(user.id)
            .fetch_optional(db.get_ref())
            .await
            .ok()
            .flatten();

    if let Some(sid) = student_id_opt {
        ctx.insert("student_id", &sid.to_string());
    } else {
        ctx.insert("student_id", "");
    }
    let current_trimester = if let Some(student_id) = student_id_opt {
        sqlx::query_scalar::<_, String>(
            "SELECT COALESCE(MAX(c.trimester), '') FROM enrollments e
             JOIN courses c ON c.id = e.course_id
             WHERE e.student_id = $1",
        )
        .bind(student_id)
        .fetch_one(db.get_ref())
        .await
        .unwrap_or_default()
    } else {
        String::new()
    };
    let current_date = chrono::Local::now().format("%A, %d %B %Y").to_string();
    ctx.insert("current_trimester", &current_trimester);
    ctx.insert("current_date", &current_date);

    #[derive(sqlx::FromRow)]
    struct DashboardStats {
        enrolled_course_count: i64,
        avg_grade: Option<f64>,
        attended_sessions: i64,
        total_sessions: i64,
        upcoming_deadlines: i64,
    }

    let stats = if let Some(student_id) = student_id_opt {
        sqlx::query_as::<_, DashboardStats>(
            "SELECT
                (SELECT COUNT(*) FROM enrollments WHERE student_id = $1) AS enrolled_course_count,
                (SELECT AVG(course_pct)::float8
                 FROM (
                    SELECT COALESCE(fg.grade::float8, items.avg_pct) AS course_pct
                    FROM enrollments e
                    JOIN courses c ON c.id = e.course_id
                    LEFT JOIN final_grades fg
                        ON fg.course_id = c.id AND fg.student_id = e.student_id
                        AND fg.released_at IS NOT NULL
                    LEFT JOIN LATERAL (
                        SELECT AVG(pct)::float8 AS avg_pct
                        FROM (
                            SELECT (s.grade::float8 / NULLIF(a.max_score, 0) * 100.0) AS pct
                            FROM assignments a
                            JOIN submissions s ON s.assignment_id = a.id
                                AND s.student_id = e.student_id
                            WHERE a.course_id = c.id AND s.status = 'graded'
                                AND s.grade IS NOT NULL
                            UNION ALL
                            SELECT (MAX(qa.score)::float8 / NULLIF(q.total_marks, 0) * 100.0) AS pct
                            FROM quizzes q
                            JOIN quiz_attempts qa ON qa.quiz_id = q.id
                                AND qa.student_id = e.student_id
                            WHERE q.course_id = c.id AND qa.submitted_at IS NOT NULL
                                AND qa.score IS NOT NULL
                            GROUP BY q.id, q.total_marks
                        ) per_item
                    ) items ON TRUE
                    WHERE e.student_id = $1
                        AND (fg.grade IS NOT NULL OR items.avg_pct IS NOT NULL)
                 ) course_scores) AS avg_grade,
                (SELECT COUNT(*)
                 FROM attendance_records ar
                 JOIN attendance_sessions s ON s.id = ar.session_id
                 JOIN enrollments e ON e.course_id = s.course_id AND e.student_id = ar.student_id
                 WHERE ar.student_id = $1 AND ar.status IN ('present', 'late', 'excused')) AS attended_sessions,
                (SELECT COUNT(*)
                 FROM attendance_sessions s
                 JOIN enrollments e ON e.course_id = s.course_id
                 WHERE e.student_id = $1) AS total_sessions,
                ((SELECT COUNT(*)
                  FROM assignments a
                  JOIN enrollments e ON e.course_id = a.course_id
                  LEFT JOIN submissions sub ON sub.assignment_id = a.id AND sub.student_id = e.student_id
                  WHERE e.student_id = $1 AND sub.id IS NULL AND a.due_date >= NOW())
                 +
                 (SELECT COUNT(*)
                  FROM quizzes q
                  JOIN enrollments e ON e.course_id = q.course_id
                  WHERE e.student_id = $1
                    AND q.is_practice = FALSE
                    AND q.close_at >= NOW()
                    AND NOT EXISTS (
                        SELECT 1 FROM quiz_attempts qa
                        WHERE qa.quiz_id = q.id AND qa.student_id = $1 AND qa.submitted_at IS NOT NULL
                    ))) AS upcoming_deadlines",
        )
        .bind(student_id)
        .fetch_one(db.get_ref())
        .await
        .unwrap_or(DashboardStats {
            enrolled_course_count: 0,
            avg_grade: None,
            attended_sessions: 0,
            total_sessions: 0,
            upcoming_deadlines: 0,
        })
    } else {
        DashboardStats {
            enrolled_course_count: 0,
            avg_grade: None,
            attended_sessions: 0,
            total_sessions: 0,
            upcoming_deadlines: 0,
        }
    };
    let avg_grade = stats.avg_grade.unwrap_or(0.0) as i32;
    let attendance_pct = if stats.total_sessions > 0 {
        ((stats.attended_sessions as f64 / stats.total_sessions as f64) * 100.0).round() as i32
    } else {
        0
    };
    ctx.insert("enrolled_course_count", &stats.enrolled_course_count);
    ctx.insert("avg_grade", &avg_grade);
    ctx.insert("attendance_pct", &attendance_pct);
    ctx.insert("upcoming_deadlines", &stats.upcoming_deadlines);

    // Sidebar active page highlight
    ctx.insert("active_page", "dashboard");

    #[derive(sqlx::FromRow)]
    struct DashboardCourseRow {
        id: i32,
        code: String,
        name: String,
        trimester: String,
        lecturer: String,
        ongoing: bool,
        attendance_pct: i32,
    }

    let course_rows = if let Some(student_id) = student_id_opt {
        sqlx::query_as::<_, DashboardCourseRow>(
            "SELECT
                c.id,
                c.course_code AS code,
                c.course_name AS name,
                COALESCE(c.trimester, '') AS trimester,
                COALESCE(u.display_name, 'TBA') AS lecturer,
                (LOWER(COALESCE(c.status, '')) = 'ongoing') AS ongoing,
                COALESCE(
                    ROUND(
                        COUNT(ar.id) FILTER (WHERE ar.status IN ('present', 'late', 'excused'))::NUMERIC
                        * 100 / NULLIF(COUNT(s.id), 0)
                    )::INT,
                    0
                ) AS attendance_pct
             FROM enrollments e
             JOIN courses c ON c.id = e.course_id
             LEFT JOIN lecturers l ON l.id = c.lecturer_id
             LEFT JOIN users u ON u.id = l.user_id
             LEFT JOIN attendance_sessions s ON s.course_id = c.id
             LEFT JOIN attendance_records ar ON ar.session_id = s.id AND ar.student_id = e.student_id
             WHERE e.student_id = $1
             GROUP BY c.id, c.course_code, c.course_name, c.trimester, c.status, u.display_name
             ORDER BY c.course_code",
        )
        .bind(student_id)
        .fetch_all(db.get_ref())
        .await
        .unwrap_or_default()
    } else {
        Vec::new()
    };

    let courses: Vec<crate::CourseContext> = course_rows
        .iter()
        .enumerate()
        .map(|(index, row)| crate::CourseContext {
            id: row.id,
            code: row.code.clone(),
            name: row.name.clone(),
            trimester: row.trimester.clone(),
            image_url: "".into(),
            pinned: index == 0,
            ongoing: row.ongoing,
            progress: row.attendance_pct,
            lecturer: row.lecturer.clone(),
            attendance_pct: row.attendance_pct,
        })
        .collect();
    ctx.insert("courses", &courses);
    let trimesters: Vec<String> = courses
        .iter()
        .map(|course| course.trimester.clone())
        .filter(|trimester| !trimester.is_empty())
        .fold(Vec::new(), |mut acc, trimester| {
            if !acc.contains(&trimester) {
                acc.push(trimester);
            }
            acc
        });
    ctx.insert("trimesters", &trimesters);

    let announcements: Vec<crate::AnnouncementContext> = if let Some(student_id) = student_id_opt {
        sqlx::query_as::<_, (String, String, String)>(
            "SELECT
                a.title,
                c.course_code || ' - ' || c.course_name AS course,
                TO_CHAR(a.created_at AT TIME ZONE 'Asia/Singapore', 'DD Mon YYYY') AS date
             FROM announcements a
             JOIN courses c ON c.id = a.course_id
             JOIN enrollments e ON e.course_id = c.id
             WHERE e.student_id = $1
             ORDER BY a.created_at DESC
             LIMIT 5",
        )
        .bind(student_id)
        .fetch_all(db.get_ref())
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|(title, course, date)| crate::AnnouncementContext { title, course, date })
        .collect()
    } else {
        Vec::new()
    };
    ctx.insert("announcements", &announcements);

    #[derive(sqlx::FromRow)]
    struct DueDateRow {
        title: String,
        course: String,
        item_type: String,
        due_date: String,
        urgent: bool,
    }

    let due_dates: Vec<crate::DueDateContext> = if let Some(student_id) = student_id_opt {
        sqlx::query_as::<_, DueDateRow>(
            "SELECT title, course, item_type, due_date, urgent
             FROM (
                SELECT
                    a.title,
                    c.course_code AS course,
                    'assignment' AS item_type,
                    TO_CHAR(a.due_date AT TIME ZONE 'Asia/Singapore', 'DD Mon') AS due_date,
                    (a.due_date - NOW() < INTERVAL '3 days') AS urgent,
                    a.due_date AS sort_at
                FROM assignments a
                JOIN enrollments e ON e.course_id = a.course_id
                JOIN courses c ON c.id = a.course_id
                LEFT JOIN submissions sub ON sub.assignment_id = a.id AND sub.student_id = e.student_id
                WHERE e.student_id = $1 AND sub.id IS NULL AND a.due_date >= NOW()
                UNION ALL
                SELECT
                    q.title,
                    c.course_code AS course,
                    'quiz' AS item_type,
                    TO_CHAR(q.close_at AT TIME ZONE 'Asia/Singapore', 'DD Mon') AS due_date,
                    (q.close_at - NOW() < INTERVAL '3 days') AS urgent,
                    q.close_at AS sort_at
                FROM quizzes q
                JOIN enrollments e ON e.course_id = q.course_id
                JOIN courses c ON c.id = q.course_id
                WHERE e.student_id = $1
                  AND q.is_practice = FALSE
                  AND q.close_at >= NOW()
                  AND NOT EXISTS (
                      SELECT 1 FROM quiz_attempts qa
                      WHERE qa.quiz_id = q.id AND qa.student_id = $1 AND qa.submitted_at IS NOT NULL
                  )
             ) due_items
             ORDER BY sort_at
             LIMIT 5",
        )
        .bind(student_id)
        .fetch_all(db.get_ref())
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|row| crate::DueDateContext {
            title: row.title,
            course: row.course,
            item_type: row.item_type,
            due_date: row.due_date,
            urgent: row.urgent,
        })
        .collect()
    } else {
        Vec::new()
    };
    ctx.insert("due_dates", &due_dates);

    let rendered = match tmpl.render("student/dashboard.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

pub async fn student_courses(
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Student) {
        Ok(u) => u,
        Err(r) => return r,
    };

    let student = match sqlx::query!("SELECT id FROM students WHERE user_id = $1", user.id)
        .fetch_optional(db.get_ref())
        .await
    {
        Ok(Some(s)) => s,
        Ok(None) => return HttpResponse::InternalServerError().body("Student profile not found"),
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };

    #[derive(serde::Serialize)]
    struct EnrolledCourse {
        id: i32,
        code: String,
        name: String,
        description: String,
        trimester: String,
        status: String,
        lecturer_name: String,
    }

    let courses = sqlx::query_as!(
        EnrolledCourse,
        r#"SELECT
            c.id,
            c.course_code                       AS code,
            c.course_name                       AS name,
            COALESCE(c.description, '')         AS "description!",
            COALESCE(c.trimester, '')           AS "trimester!",
            COALESCE(c.status, 'Preparing')     AS "status!",
            COALESCE(u.display_name, 'TBA')     AS "lecturer_name!"
           FROM enrollments e
           JOIN courses c        ON c.id = e.course_id
           LEFT JOIN lecturers l ON l.id = c.lecturer_id
           LEFT JOIN users u     ON u.id = l.user_id
           WHERE e.student_id = $1
           ORDER BY c.course_code ASC"#,
        student.id
    )
    .fetch_all(db.get_ref())
    .await
    .unwrap_or_default();

    let total = courses.len();
    let mut ctx = Context::new();
    crate::insert_student_base(&mut ctx, &user.display_name, "");
    ctx.insert("active_page", "courses");
    ctx.insert("courses", &courses);
    ctx.insert("total_courses", &total);

    let rendered = match tmpl.render("student/courses.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

// ── ASSIGNMENTS ───────────────────────────────────────────────────────────────

pub async fn student_assignments(
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Student) {
        Ok(u) => u,
        Err(r) => return r,
    };

    let student = match sqlx::query!("SELECT id FROM students WHERE user_id = $1", user.id)
        .fetch_optional(db.get_ref())
        .await
    {
        Ok(Some(s)) => s,
        Ok(None) => return HttpResponse::Forbidden().body("No student profile"),
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };

    #[derive(serde::Serialize)]
    struct CourseRow {
        id: i32,
        code: String,
        name: String,
        description: String,
        status: String,
        trimester: String,
        assignment_count: i64,
    }

    let courses = match sqlx::query_as!(
        CourseRow,
        r#"SELECT
            c.id,
            c.course_code                       AS code,
            c.course_name                       AS name,
            COALESCE(c.description, '')         AS "description!",
            COALESCE(c.status, 'Ongoing')       AS "status!",
            COALESCE(c.trimester, '')           AS "trimester!",
            COUNT(a.id)                         AS "assignment_count!"
           FROM enrollments e
           JOIN courses c ON c.id = e.course_id
           JOIN assignments a ON a.course_id = c.id
           WHERE e.student_id = $1
           GROUP BY c.id, c.course_code, c.course_name, c.description, c.status, c.trimester
           ORDER BY c.course_code"#,
        student.id
    )
    .fetch_all(db.get_ref())
    .await
    {
        Ok(r) => r,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };

    let total_courses = courses.len();
    let total_assignments: i64 = courses.iter().map(|c| c.assignment_count).sum();

    let mut ctx = Context::new();
    crate::insert_student_base(&mut ctx, &user.display_name, "");
    ctx.insert("active_page", "assignments");
    ctx.insert("courses", &courses);
    ctx.insert("total_courses", &total_courses);
    ctx.insert("total_assignments", &total_assignments);

    let rendered = match tmpl.render("student/assignments.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

// ── ASSIGNMENTS DATA (JSON) ───────────────────────────────────────────────────

pub async fn student_assignments_data(
    db: web::Data<PgPool>,
    session: Session,
    query: web::Query<std::collections::HashMap<String, String>>,
    storage: web::Data<crate::storage::SupabaseStorage>,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Student) {
        Ok(u) => u,
        Err(r) => return r,
    };

    let student = match sqlx::query!("SELECT id FROM students WHERE user_id = $1", user.id)
        .fetch_optional(db.get_ref())
        .await
    {
        Ok(Some(s)) => s,
        _ => return HttpResponse::Forbidden().finish(),
    };

    let course_id: i32 = match query.get("course_id").and_then(|v| v.parse().ok()) {
        Some(id) => id,
        None => return HttpResponse::BadRequest().body("Missing course_id"),
    };

    // Verify enrollment
    let enrolled = sqlx::query!(
        "SELECT 1 AS one FROM enrollments WHERE student_id = $1 AND course_id = $2",
        student.id,
        course_id
    )
    .fetch_optional(db.get_ref())
    .await
    .ok()
    .flatten();

    if enrolled.is_none() {
        return HttpResponse::Forbidden().finish();
    }

    #[derive(serde::Serialize, sqlx::FromRow)]
    struct AsgRow {
        id: i32,
        week_number: Option<i32>,
        title: String,
        description: String,
        due_date: chrono::DateTime<chrono::Utc>,
        max_score: i32,
        file_count: Option<i64>,
    }

    #[derive(serde::Serialize, sqlx::FromRow)]
    struct FileRow {
        id: i32,
        file_name: String,
        file_path: String,
    }

    #[derive(serde::Serialize, sqlx::FromRow)]
    struct StudentSubmissionRow {
        id: i32,
        file_path: String,
        submitted_at: chrono::DateTime<chrono::Utc>,
        status: String,
        grade: Option<f64>,
        feedback: Option<String>,
    }

    let assignments = match sqlx::query_as!(
        AsgRow,
        r#"SELECT a.id, a.week_number, a.title,
                  COALESCE(a.description, '') AS "description!",
                  a.due_date, a.max_score,
                  COUNT(af.id) AS file_count
           FROM assignments a
           LEFT JOIN assignment_files af ON af.assignment_id = a.id
           WHERE a.course_id = $1
           GROUP BY a.id
           ORDER BY a.week_number NULLS LAST, a.due_date"#,
        course_id
    )
    .fetch_all(db.get_ref())
    .await
    {
        Ok(r) => r,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };

    let mut result = Vec::new();
    for a in assignments {
        let raw_files = sqlx::query_as!(
            FileRow,
            "SELECT id, file_name, file_path FROM assignment_files WHERE assignment_id = $1",
            a.id
        )
        .fetch_all(db.get_ref())
        .await
        .unwrap_or_default();

        let files_with_url: Vec<serde_json::Value> = raw_files
            .into_iter()
            .map(|f| {
                serde_json::json!({
                    "id": f.id,
                    "file_name": f.file_name,
                    "file_url": storage.public_url(&f.file_path),  // ← storage, not storage_ref
                })
            })
            .collect();

        let submission = match sqlx::query_as::<_, StudentSubmissionRow>(
            "SELECT id, file_path, submitted_at, status, grade::float8 AS grade, feedback
             FROM submissions
             WHERE assignment_id = $1 AND student_id = $2
             ORDER BY submitted_at DESC
             LIMIT 1",
        )
        .bind(a.id)
        .bind(student.id)
        .fetch_optional(db.get_ref())
        .await
        {
            Ok(Some(s)) => Some(serde_json::json!({
                "id": s.id,
                "file_name": storage_filename(&s.file_path),
                "file_url": storage.public_url(&s.file_path),
                "submitted_at": s.submitted_at,
                "status": s.status,
                "grade": s.grade,
                "feedback": s.feedback,
            })),
            Ok(None) => None,
            Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
        };

        result.push(serde_json::json!({
            "id": a.id,
            "week_number": a.week_number,
            "title": a.title,
            "description": a.description,
            "due_date": a.due_date,
            "max_score": a.max_score,
            "file_count": a.file_count,
            "files": files_with_url,
            "submission": submission,
        }));
    }

    HttpResponse::Ok().json(result)
}

pub async fn student_grades(
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Student) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    let student = match sqlx::query!("SELECT id FROM students WHERE user_id = $1", user.id)
        .fetch_optional(db.get_ref())
        .await
    {
        Ok(Some(s)) => s,
        Ok(None) => {
            crate::insert_student_base(&mut ctx, &user.display_name, "");
            ctx.insert("active_page", "grades");
            ctx.insert("course_grades", &Vec::<crate::CourseGradeContext>::new());
            ctx.insert("overall_avg", &0);
            ctx.insert("highest_grade", &0);
            ctx.insert("at_risk_count", &0usize);
            let rendered = match tmpl.render("student/grades.html", &ctx) {
                Ok(html) => html,
                Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
            };
            return HttpResponse::Ok().content_type("text/html").body(rendered);
        }
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };

    crate::insert_student_base(&mut ctx, &user.display_name, &student.id.to_string());
    ctx.insert("active_page", "grades");

    #[derive(sqlx::FromRow)]
    struct CourseGradeRow {
        id: i32,
        code: String,
        name: String,
        overall: Option<f64>,
        grade_scale: Option<String>,
    }

    #[derive(sqlx::FromRow)]
    struct GradeItemRow {
        title: String,
        item_type: String,
        score: f64,
        max_score: f64,
    }

    let grade_rows = sqlx::query_as::<_, CourseGradeRow>(
        "SELECT
            c.id,
            c.course_code AS code,
            c.course_name AS name,
            fg.grade::float8 AS overall,
            fg.grade_scale
         FROM enrollments e
         JOIN courses c ON c.id = e.course_id
         LEFT JOIN final_grades fg
            ON fg.course_id = c.id AND fg.student_id = e.student_id AND fg.released_at IS NOT NULL
         WHERE e.student_id = $1
         ORDER BY c.course_code",
    )
    .bind(student.id)
    .fetch_all(db.get_ref())
    .await
    .unwrap_or_default();

    let mut course_grades: Vec<crate::CourseGradeContext> = Vec::new();
    for course in grade_rows {
        let items = sqlx::query_as::<_, GradeItemRow>(
            "SELECT title, item_type, score, max_score
             FROM (
                SELECT
                    a.title,
                    'assignment' AS item_type,
                    COALESCE(s.grade, 0)::float8 AS score,
                    a.max_score::float8 AS max_score,
                    COALESCE(s.submitted_at, a.due_date) AS sort_at
                FROM assignments a
                JOIN submissions s ON s.assignment_id = a.id AND s.student_id = $1
                WHERE a.course_id = $2 AND s.status = 'graded' AND s.grade IS NOT NULL
                UNION ALL
                SELECT
                    q.title,
                    'quiz' AS item_type,
                    MAX(COALESCE(qa.score, 0))::float8 AS score,
                    q.total_marks::float8 AS max_score,
                    MAX(COALESCE(qa.submitted_at, q.close_at)) AS sort_at
                FROM quizzes q
                JOIN quiz_attempts qa ON qa.quiz_id = q.id AND qa.student_id = $1
                WHERE q.course_id = $2 AND q.is_practice = FALSE AND qa.submitted_at IS NOT NULL AND qa.score IS NOT NULL
                GROUP BY q.id, q.title, q.total_marks
             ) grade_items
             ORDER BY sort_at",
        )
        .bind(student.id)
        .bind(course.id)
        .fetch_all(db.get_ref())
        .await
        .unwrap_or_default();

        if course.overall.is_none() && items.is_empty() {
            continue;
        }

        // Equal-split weighting: every graded component carries an equal
        // share of 100%, so the weights always sum to 100%.
        let item_weight = if items.is_empty() {
            0.0
        } else {
            100.0 / items.len() as f32
        };

        let item_contexts: Vec<crate::GradeItemContext> = items
            .into_iter()
            .map(|item| crate::GradeItemContext {
                title: item.title,
                item_type: item.item_type,
                score: item.score as f32,
                max_score: item.max_score as f32,
                weight: item_weight,
            })
            .collect();

        // Course overall is the weighted average of each component's
        // percentage score. With equal weights this matches a plain average.
        let overall = course.overall.map(|g| g as f32).unwrap_or_else(|| {
            item_contexts
                .iter()
                .map(|item| {
                    let pct = if item.max_score > 0.0 {
                        item.score / item.max_score * 100.0
                    } else {
                        0.0
                    };
                    pct * item.weight / 100.0
                })
                .sum::<f32>()
        });

        course_grades.push(crate::CourseGradeContext {
            code: course.code,
            name: course.name,
            overall,
            grade_letter: course
                .grade_scale
                .unwrap_or_else(|| grade_letter_from_percentage(overall).to_string()),
            items: item_contexts,
        });
    }

    // Derived summary stats
    let overall_avg = if course_grades.is_empty() {
        0
    } else {
        (course_grades.iter().map(|c| c.overall).sum::<f32>() / course_grades.len() as f32) as i32
    };
    let highest_grade = course_grades
        .iter()
        .map(|c| c.overall as i32)
        .max()
        .unwrap_or(0);
    let at_risk_count = course_grades.iter().filter(|c| c.overall < 60.0).count();

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

fn grade_letter_from_percentage(score: f32) -> &'static str {
    if score >= 85.0 {
        "A"
    } else if score >= 80.0 {
        "A-"
    } else if score >= 75.0 {
        "B+"
    } else if score >= 70.0 {
        "B"
    } else if score >= 65.0 {
        "B-"
    } else if score >= 60.0 {
        "C+"
    } else if score >= 55.0 {
        "C"
    } else if score >= 50.0 {
        "D"
    } else {
        "F"
    }
}

pub async fn student_announcement(
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Student) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let mut ctx = Context::new();
    let student = match sqlx::query!("SELECT id FROM students WHERE user_id = $1", user.id)
        .fetch_optional(db.get_ref())
        .await
    {
        Ok(Some(s)) => s,
        Ok(None) => {
            crate::insert_student_base(&mut ctx, &user.display_name, "");
            ctx.insert("active_page", "announcements");
            ctx.insert("courses", &Vec::<crate::CourseContext>::new());
            ctx.insert("announcements", &Vec::<crate::AnnouncementFullContext>::new());
            let rendered = match tmpl.render("student/announcement.html", &ctx) {
                Ok(html) => html,
                Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
            };
            return HttpResponse::Ok().content_type("text/html").body(rendered);
        }
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };

    crate::insert_student_base(&mut ctx, &user.display_name, &student.id.to_string());
    ctx.insert("active_page", "announcements");

    #[derive(sqlx::FromRow)]
    struct AnnouncementCourseRow {
        id: i32,
        code: String,
        name: String,
        trimester: String,
        lecturer: String,
        ongoing: bool,
    }

    let course_rows = sqlx::query_as::<_, AnnouncementCourseRow>(
        "SELECT
            c.id,
            c.course_code AS code,
            c.course_name AS name,
            COALESCE(c.trimester, '') AS trimester,
            COALESCE(u.display_name, 'TBA') AS lecturer,
            (LOWER(COALESCE(c.status, '')) = 'ongoing') AS ongoing
         FROM enrollments e
         JOIN courses c ON c.id = e.course_id
         LEFT JOIN lecturers l ON l.id = c.lecturer_id
         LEFT JOIN users u ON u.id = l.user_id
         WHERE e.student_id = $1
         ORDER BY c.course_code",
    )
    .bind(student.id)
    .fetch_all(db.get_ref())
    .await
    .unwrap_or_default();

    let courses: Vec<crate::CourseContext> = course_rows
        .into_iter()
        .map(|row| crate::CourseContext {
            id: row.id,
            code: row.code,
            name: row.name,
            trimester: row.trimester,
            image_url: "".into(),
            pinned: false,
            ongoing: row.ongoing,
            progress: 0,
            lecturer: row.lecturer,
            attendance_pct: 0,
        })
        .collect();
    ctx.insert("courses", &courses);

    #[derive(sqlx::FromRow)]
    struct AnnouncementRow {
        id: i32,
        title: String,
        course: String,
        course_code: String,
        date: String,
        content: String,
        is_new: bool,
    }

    let announcements: Vec<crate::AnnouncementFullContext> =
        sqlx::query_as::<_, AnnouncementRow>(
            "SELECT
                a.id,
                a.title,
                c.course_name AS course,
                c.course_code,
                TO_CHAR(a.created_at AT TIME ZONE 'Asia/Singapore', 'DD Mon YYYY') AS date,
                a.content,
                (a.created_at >= NOW() - INTERVAL '7 days') AS is_new
             FROM announcements a
             JOIN courses c ON c.id = a.course_id
             JOIN enrollments e ON e.course_id = c.id
             WHERE e.student_id = $1
             ORDER BY a.created_at DESC",
        )
        .bind(student.id)
        .fetch_all(db.get_ref())
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|row| crate::AnnouncementFullContext {
            id: row.id,
            title: row.title,
            course: row.course,
            course_code: row.course_code,
            date: row.date,
            content: row.content,
            is_new: row.is_new,
        })
        .collect();
    ctx.insert("announcements", &announcements);

    let rendered = match tmpl.render("student/announcement.html", &ctx) {
        Ok(html) => html,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}


pub async fn student_profile_page(
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Student) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let profile = match sqlx::query_as::<_, StudentProfileDetails>(
        "SELECT
             u.display_name,
             u.email,
             u.role,
             u.is_active,
             to_char(u.created_at AT TIME ZONE 'Asia/Singapore', 'YYYY-MM-DD HH24:MI:SS') AS created_at,
             s.id AS student_id,
             s.age,
             s.programme,
             s.year_of_study,
             COUNT(e.id)::BIGINT AS enrolled_courses
         FROM users u
         JOIN students s ON s.user_id = u.id
         LEFT JOIN enrollments e ON e.student_id = s.id
         WHERE u.id = $1
         GROUP BY u.id, s.id",
    )
    .bind(user.id)
    .fetch_optional(db.get_ref())
    .await
    {
        Ok(Some(profile)) => profile,
        Ok(None) => return HttpResponse::InternalServerError().body("Student profile not found"),
        Err(error) => return HttpResponse::InternalServerError().body(error.to_string()),
    };

    let mut ctx = Context::new();
    crate::insert_student_base(&mut ctx, &user.display_name, &profile.student_id.to_string());
    ctx.insert("active_page", "profile");
    ctx.insert("profile", &profile);

    let rendered = match tmpl.render("student/profile.html", &ctx) {
        Ok(html) => html,
        Err(error) => return HttpResponse::InternalServerError().body(error.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

pub async fn student_settings_page(
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Student) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let preferences = match load_user_preferences(db.get_ref(), user.id).await {
        Ok(preferences) => preferences,
        Err(error) => return HttpResponse::InternalServerError().body(error.to_string()),
    };

    let student_id = sqlx::query_scalar::<_, i32>("SELECT id FROM students WHERE user_id = $1")
        .bind(user.id)
        .fetch_optional(db.get_ref())
        .await
        .ok()
        .flatten()
        .map(|id| id.to_string())
        .unwrap_or_default();

    let mut ctx = Context::new();
    crate::insert_student_base(&mut ctx, &user.display_name, &student_id);
    ctx.insert("active_page", "settings");
    ctx.insert("preferences", &preferences);
    if let Ok(Some(message)) = session.get::<String>("settings_success") {
        ctx.insert("settings_success", &message);
        let _ = session.remove("settings_success");
    }

    let rendered = match tmpl.render("student/settings.html", &ctx) {
        Ok(html) => html,
        Err(error) => return HttpResponse::InternalServerError().body(error.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

pub async fn student_settings_submit(
    db: web::Data<PgPool>,
    session: Session,
    form: web::Form<UserPreferencesForm>,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Student) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let form = form.into_inner();
    let theme_mode = if form.theme_mode == "dark" { "dark" } else { "light" };

    if let Err(error) = ensure_user_preferences_table(db.get_ref()).await {
        return HttpResponse::InternalServerError().body(error.to_string());
    }

    let result = sqlx::query(
        "INSERT INTO user_preferences (
             user_id, email_notifications, course_notifications, forum_notifications,
             grade_notifications, theme_mode, updated_at
         )
         VALUES ($1, $2, $3, $4, $5, $6, NOW())
         ON CONFLICT (user_id) DO UPDATE
         SET email_notifications = EXCLUDED.email_notifications,
             course_notifications = EXCLUDED.course_notifications,
             forum_notifications = EXCLUDED.forum_notifications,
             grade_notifications = EXCLUDED.grade_notifications,
             theme_mode = EXCLUDED.theme_mode,
             updated_at = NOW()",
    )
    .bind(user.id)
    .bind(form.email_notifications.is_some())
    .bind(form.course_notifications.is_some())
    .bind(form.forum_notifications.is_some())
    .bind(form.grade_notifications.is_some())
    .bind(theme_mode)
    .execute(db.get_ref())
    .await;

    if let Err(error) = result {
        return HttpResponse::InternalServerError().body(error.to_string());
    }

    let actor = AuditActor {
        user_id: Some(user.id),
        role: Some("student".to_string()),
        display_name: None,
    };
    log_audit_event(
        db.get_ref(),
        "settings",
        "student_settings_saved",
        "info",
        &actor,
        Some("preferences"),
        Some(user.id),
        Some(format!("Theme set to {theme_mode}")),
    )
    .await;

    let _ = session.insert("settings_success", "Settings saved.");
    let cookie_val = format!("lms-theme={}; Path=/; Max-Age=31536000; SameSite=Lax", theme_mode);
    HttpResponse::SeeOther()
        .insert_header((actix_web::http::header::LOCATION, "/student/settings"))
        .insert_header((actix_web::http::header::SET_COOKIE, cookie_val))
        .finish()
}

pub async fn student_course_data(
    cid: web::Path<i32>,
    db: web::Data<PgPool>,
    session: Session,
    storage: web::Data<crate::storage::SupabaseStorage>,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Student) {
        Ok(u) => u,
        Err(r) => return r,
    };

    let course_id = cid.into_inner();

    let student = match sqlx::query!("SELECT id FROM students WHERE user_id = $1", user.id)
        .fetch_optional(db.get_ref())
        .await
    {
        Ok(Some(s)) => s,
        _ => return HttpResponse::Forbidden().body("Student profile not found"),
    };

    let enrolled = sqlx::query!(
        "SELECT 1 AS exists FROM enrollments WHERE student_id = $1 AND course_id = $2",
        student.id,
        course_id
    )
    .fetch_optional(db.get_ref())
    .await
    .ok()
    .flatten();

    if enrolled.is_none() {
        return HttpResponse::Forbidden().body("You are not enrolled in this course");
    }

    let weeks_raw = match sqlx::query!(
        "SELECT id, week_number, title FROM course_weeks
         WHERE course_id = $1 ORDER BY week_number ASC",
        course_id
    )
    .fetch_all(db.get_ref())
    .await
    {
        Ok(r) => r,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };

    let mut weeks = Vec::new();
    for w in weeks_raw {
        let files = sqlx::query!(
            "SELECT id, title, file_path FROM course_materials
             WHERE week_id = $1 ORDER BY uploaded_at ASC",
            w.id
        )
        .fetch_all(db.get_ref())
        .await
        .unwrap_or_default();

        let file_list: Vec<serde_json::Value> = files
            .iter()
            .map(|f| {
                let url = storage.public_url(&f.file_path);
                serde_json::json!({
                    "id": f.id,
                    "title": f.title,
                    "file_url": url
                })
            })
            .collect();

        weeks.push(serde_json::json!({
            "id": w.id,
            "week_number": w.week_number,
            "title": w.title,
            "files": file_list
        }));
    }

    HttpResponse::Ok().json(serde_json::json!({ "weeks": weeks }))
}

pub async fn student_assignment_submit(
    db: web::Data<PgPool>,
    storage: web::Data<crate::storage::SupabaseStorage>,
    session: Session,
    mut payload: actix_multipart::Multipart,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Student) {
        Ok(u) => u,
        Err(r) => return r,
    };

    let student = match sqlx::query!("SELECT id FROM students WHERE user_id = $1", user.id)
        .fetch_optional(db.get_ref())
        .await
    {
        Ok(Some(s)) => s,
        _ => return HttpResponse::Forbidden().body("No student profile"),
    };

    let mut assignment_id: Option<i32> = None;
    let mut notes: Option<String> = None;
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut file_name: Option<String> = None;
    let mut content_type = "application/pdf".to_string();

    use futures_util::TryStreamExt;
    while let Ok(Some(mut field)) = payload.try_next().await {
        let cd = field.content_disposition(); // Option<&ContentDisposition>
        let name = cd.and_then(|cd| cd.get_name()).unwrap_or("").to_string();
        let field_filename = cd.and_then(|cd| cd.get_filename()).map(|s| s.to_string());

        match name.as_str() {
            "assignment_id" => {
                let mut bytes = Vec::new();
                while let Ok(Some(chunk)) = field.try_next().await {
                    bytes.extend_from_slice(&chunk);
                }
                assignment_id = String::from_utf8_lossy(&bytes).trim().parse().ok();
            }
            "notes" => {
                let mut bytes = Vec::new();
                while let Ok(Some(chunk)) = field.try_next().await {
                    bytes.extend_from_slice(&chunk);
                }
                let value = String::from_utf8_lossy(&bytes).trim().to_string();
                if !value.is_empty() {
                    notes = Some(value);
                }
            }
            "file" => {
                file_name = field_filename;
                content_type = field
                    .content_type()
                    .map(|m| m.to_string())
                    .unwrap_or_else(|| "application/pdf".to_string());

                let mut bytes = Vec::new();
                while let Ok(Some(chunk)) = field.try_next().await {
                    bytes.extend_from_slice(&chunk);
                    if bytes.len() > 20 * 1024 * 1024 {
                        return HttpResponse::BadRequest().body("File must be 20 MB or smaller");
                    }
                }
                file_bytes = Some(bytes);
            }
            _ => while let Ok(Some(_)) = field.try_next().await {},
        }
    }

    let assignment_id = match assignment_id {
        Some(id) => id,
        None => return HttpResponse::BadRequest().body("Missing assignment_id"),
    };
    let file_bytes = match file_bytes {
        Some(b) if !b.is_empty() => b,
        _ => return HttpResponse::BadRequest().body("No file uploaded"),
    };
    let file_name =
        sanitize_storage_filename(&file_name.unwrap_or_else(|| "submission.pdf".to_string()));
    let file_name = if file_name.is_empty() {
        "submission.pdf".to_string()
    } else {
        file_name
    };
    if !file_name.to_ascii_lowercase().ends_with(".pdf") {
        return HttpResponse::BadRequest().body("Only PDF submissions are accepted");
    }

    #[derive(sqlx::FromRow)]
    struct AssignmentTarget {
        course_id: i32,
        due_date: chrono::DateTime<chrono::Utc>,
    }

    let asg = match sqlx::query_as::<_, AssignmentTarget>(
        "SELECT a.course_id, a.due_date FROM assignments a
         JOIN enrollments e ON e.course_id = a.course_id
         WHERE a.id = $1 AND e.student_id = $2",
    )
    .bind(assignment_id)
    .bind(student.id)
    .fetch_optional(db.get_ref())
    .await
    {
        Ok(Some(r)) => r,
        Ok(None) => return HttpResponse::Forbidden().body("Assignment not found or not enrolled"),
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };

    let file_path = format!(
        "submissions/{}/{}/student_{}_{}",
        asg.course_id, assignment_id, student.id, file_name
    );

    #[derive(sqlx::FromRow)]
    struct ExistingSubmission {
        file_path: String,
    }

    if let Some(old_submission) = sqlx::query_as::<_, ExistingSubmission>(
        "SELECT file_path FROM submissions WHERE assignment_id = $1 AND student_id = $2",
    )
    .bind(assignment_id)
    .bind(student.id)
    .fetch_optional(db.get_ref())
    .await
    .ok()
    .flatten()
    {
        let _ = storage.delete(&old_submission.file_path).await;
        let _ = sqlx::query("DELETE FROM submissions WHERE assignment_id = $1 AND student_id = $2")
            .bind(assignment_id)
            .bind(student.id)
            .execute(db.get_ref())
            .await;
    }

    if storage.base_url.is_empty()
        || storage.bucket.is_empty()
        || storage.service_role_key.is_empty()
    {
        return HttpResponse::InternalServerError()
            .body("Supabase Storage is not configured in .env.");
    }

    if let Err(e) = storage.upload(&file_path, file_bytes, &content_type).await {
        return HttpResponse::InternalServerError().body(format!("Upload failed: {e}"));
    }

    let status = if chrono::Utc::now() > asg.due_date {
        "late"
    } else {
        "submitted"
    };

    let result = sqlx::query(
        "INSERT INTO submissions
            (assignment_id, student_id, file_path, submitted_at, status, feedback)
         VALUES ($1, $2, $3, NOW(), $4, $5)",
    )
    .bind(assignment_id)
    .bind(student.id)
    .bind(file_path)
    .bind(status)
    .bind(notes)
    .execute(db.get_ref())
    .await;

    match result {
        Ok(_) => HttpResponse::Ok().body("submitted"),
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
    }
}

fn sanitize_storage_filename(filename: &str) -> String {
    let sanitized: String = filename
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect();

    sanitized.trim_matches('_').trim_matches('.').to_string()
}

fn storage_filename(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}
