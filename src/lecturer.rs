use actix_multipart::Multipart;
use actix_session::Session;
use actix_web::{HttpResponse, Responder, web};
use futures_util::StreamExt;
use serde::Serialize;
use serde_json::json;
use sqlx::PgPool;
use tera::{Context, Tera};

use crate::admin::{AuditActor, log_audit_event};
use crate::auth::UserRole;

#[derive(Serialize, sqlx::FromRow)]
struct LecturerProfileDetails {
    display_name: String,
    email: String,
    role: String,
    is_active: bool,
    created_at: String,
    lecturer_id: i32,
    staff_no: String,
    department: String,
    assigned_courses: i64,
}

#[derive(Serialize, sqlx::FromRow)]
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

// ─── helper: get lecturer row from session ───────────────────────────────────
async fn get_lecturer(
    session: &Session,
    db: &PgPool,
) -> Result<(crate::auth::CurrentUser, i32), HttpResponse> {
    let user = match crate::auth::require_role(session, UserRole::Lecturer) {
        Ok(u) => u,
        Err(r) => return Err(r),
    };
    let row = match sqlx::query!("SELECT id FROM lecturers WHERE user_id = $1", user.id)
        .fetch_optional(db)
        .await
    {
        Ok(Some(r)) => r,
        Ok(None) => return Err(HttpResponse::Forbidden().body("No lecturer profile")),
        Err(e) => return Err(HttpResponse::InternalServerError().body(e.to_string())),
    };
    Ok((user, row.id))
}

// ─── base context helper ─────────────────────────────────────────────────────
fn base_ctx(user: &crate::auth::CurrentUser, active: &str) -> Context {
    let mut ctx = Context::new();
    ctx.insert("display_name", &user.display_name);
    ctx.insert("student_name", &user.display_name);
    ctx.insert("student_id", "");
    ctx.insert("notifications", &Vec::<crate::NotificationContext>::new());
    ctx.insert("active_page", active);
    ctx.insert("is_lecturer", &true);
    ctx
}

// ─── page handlers ────────────────────────────────────────────────────────────

pub async fn lecturer_dashboard(
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    let (user, lecturer_id) = match get_lecturer(&session, db.get_ref()).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let mut ctx = base_ctx(&user, "dashboard");

    #[derive(Serialize, sqlx::FromRow)]
    struct LecturerDashboardCourse {
        code: String,
        name: String,
        term: String,
        students: i64,
        status: String,
    }

    #[derive(Serialize, sqlx::FromRow)]
    struct PendingSubmissionSummary {
        title: String,
        course: String,
        submitted_by: String,
        due: String,
        pending_count: i64,
    }

    #[derive(Serialize, sqlx::FromRow)]
    struct ForumQuestionSummary {
        title: String,
        course: String,
        author: String,
        when: String,
    }

    #[derive(Serialize, sqlx::FromRow)]
    struct UpcomingEvent {
        title: String,
        course: String,
        when: String,
    }

    let assigned_courses = sqlx::query_as::<_, LecturerDashboardCourse>(
        "SELECT
            c.course_code AS code,
            c.course_name AS name,
            COALESCE(c.trimester, '') AS term,
            COUNT(DISTINCT e.student_id) AS students,
            COALESCE(c.status, 'Preparing') AS status
         FROM courses c
         LEFT JOIN enrollments e ON e.course_id = c.id
         WHERE c.lecturer_id = $1
         GROUP BY c.id, c.course_code, c.course_name, c.trimester, c.status
         ORDER BY c.course_code",
    )
    .bind(lecturer_id)
    .fetch_all(db.get_ref())
    .await
    .unwrap_or_default();

    let assigned_courses_count = assigned_courses.len();
    let student_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(DISTINCT e.student_id)
         FROM enrollments e
         JOIN courses c ON c.id = e.course_id
         WHERE c.lecturer_id = $1",
    )
    .bind(lecturer_id)
    .fetch_one(db.get_ref())
    .await
    .unwrap_or(0);
    let pending_grades_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)
         FROM submissions s
         JOIN assignments a ON a.id = s.assignment_id
         JOIN courses c ON c.id = a.course_id
         WHERE c.lecturer_id = $1 AND s.status IN ('submitted', 'late')",
    )
    .bind(lecturer_id)
    .fetch_one(db.get_ref())
    .await
    .unwrap_or(0);
    let forum_questions_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)
         FROM forum_threads ft
         JOIN courses c ON c.id = ft.course_id
         WHERE c.lecturer_id = $1
           AND ft.deleted_at IS NULL
           AND ft.is_answered = FALSE",
    )
    .bind(lecturer_id)
    .fetch_one(db.get_ref())
    .await
    .unwrap_or(0);

    let pending_submissions = sqlx::query_as::<_, PendingSubmissionSummary>(
        "SELECT
            a.title,
            c.course_code AS course,
            COALESCE(MAX(u.display_name), 'Student') AS submitted_by,
            TO_CHAR(a.due_date AT TIME ZONE 'Asia/Singapore', 'DD Mon YYYY') AS due,
            COUNT(s.id) AS pending_count
         FROM submissions s
         JOIN assignments a ON a.id = s.assignment_id
         JOIN courses c ON c.id = a.course_id
         JOIN students st ON st.id = s.student_id
         JOIN users u ON u.id = st.user_id
         WHERE c.lecturer_id = $1 AND s.status IN ('submitted', 'late')
         GROUP BY a.id, a.title, c.course_code, a.due_date
         ORDER BY MIN(s.submitted_at) DESC
         LIMIT 5",
    )
    .bind(lecturer_id)
    .fetch_all(db.get_ref())
    .await
    .unwrap_or_default();

    let forum_questions = sqlx::query_as::<_, ForumQuestionSummary>(
        "SELECT
            ft.title,
            c.course_code AS course,
            u.display_name AS author,
            TO_CHAR(ft.created_at AT TIME ZONE 'Asia/Singapore', 'DD Mon, HH24:MI') AS when
         FROM forum_threads ft
         JOIN courses c ON c.id = ft.course_id
         JOIN users u ON u.id = ft.created_by
         WHERE c.lecturer_id = $1
           AND ft.deleted_at IS NULL
           AND ft.is_answered = FALSE
         ORDER BY ft.created_at DESC
         LIMIT 5",
    )
    .bind(lecturer_id)
    .fetch_all(db.get_ref())
    .await
    .unwrap_or_default();

    let upcoming_events = sqlx::query_as::<_, UpcomingEvent>(
        "SELECT title, course, when
         FROM (
            SELECT
                a.title,
                c.course_code AS course,
                TO_CHAR(a.due_date AT TIME ZONE 'Asia/Singapore', 'DD Mon YYYY') AS when,
                a.due_date AS sort_at
            FROM assignments a
            JOIN courses c ON c.id = a.course_id
            WHERE c.lecturer_id = $1 AND a.due_date >= NOW()
            UNION ALL
            SELECT
                q.title,
                c.course_code AS course,
                TO_CHAR(q.close_at AT TIME ZONE 'Asia/Singapore', 'DD Mon YYYY') AS when,
                q.close_at AS sort_at
            FROM quizzes q
            JOIN courses c ON c.id = q.course_id
            WHERE c.lecturer_id = $1 AND q.close_at >= NOW()
         ) upcoming
         ORDER BY sort_at
         LIMIT 5",
    )
    .bind(lecturer_id)
    .fetch_all(db.get_ref())
    .await
    .unwrap_or_default();

    ctx.insert("assigned_courses_count", &assigned_courses_count);
    ctx.insert("student_count", &student_count);
    ctx.insert("pending_grades_count", &pending_grades_count);
    ctx.insert("forum_questions_count", &forum_questions_count);
    ctx.insert("assigned_courses", &assigned_courses);
    ctx.insert("pending_submissions", &pending_submissions);
    ctx.insert("forum_questions", &forum_questions);
    ctx.insert("upcoming_events", &upcoming_events);
    let rendered = match tmpl.render("lecturer/dashboard.html", &ctx) {
        Ok(h) => h,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

// ─── COURSES LIST ─────────────────────────────────────────────────────────────

pub async fn lecturer_courses_page(
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    let (user, lecturer_id) = match get_lecturer(&session, db.get_ref()).await {
        Ok(v) => v,
        Err(r) => return r,
    };

    #[derive(Serialize)]
    struct EnrolledStudentRow {
        id: i32,
        display_name: String,
        email: String,
        programme: String,
        year_of_study: Option<i32>,
        enrolled_at: String,
    }

    #[derive(Serialize)]
    struct CourseRow {
        id: i32,
        code: String,
        name: String,
        description: String,
        status: String,
        trimester: String,
        enrolled_count: i64,
        students: Vec<EnrolledStudentRow>,
    }

    #[derive(Serialize)]
    struct CourseSummaryRow {
        id: i32,
        code: String,
        name: String,
        description: String,
        status: String,
        trimester: String,
        enrolled_count: i64,
    }

    let course_rows = match sqlx::query_as!(
        CourseSummaryRow,
        r#"SELECT id,
                  course_code                          AS code,
                  course_name                          AS name,
                  COALESCE(description, '')            AS "description!",
                  COALESCE(status, 'Preparing')        AS "status!",
                  COALESCE(trimester, '')              AS "trimester!",
                  (
                      SELECT COUNT(*)
                      FROM enrollments e
                      WHERE e.course_id = courses.id
                  )                                   AS "enrolled_count!"
           FROM courses
           WHERE lecturer_id = $1
           ORDER BY created_at DESC"#,
        lecturer_id
    )
    .fetch_all(db.get_ref())
    .await
    {
        Ok(r) => r,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };

    let mut courses = Vec::with_capacity(course_rows.len());
    for course in course_rows {
        let students = match sqlx::query_as!(
            EnrolledStudentRow,
            r#"SELECT
                   s.id,
                   u.display_name,
                   u.email,
                   COALESCE(s.programme, '') AS "programme!",
                   s.year_of_study,
                   TO_CHAR(e.enrolled_at AT TIME ZONE 'Asia/Singapore', 'DD Mon YYYY') AS "enrolled_at!"
               FROM enrollments e
               JOIN students s ON s.id = e.student_id
               JOIN users u ON u.id = s.user_id
               JOIN courses c ON c.id = e.course_id
               WHERE e.course_id = $1 AND c.lecturer_id = $2
               ORDER BY u.display_name ASC"#,
            course.id,
            lecturer_id
        )
        .fetch_all(db.get_ref())
        .await
        {
            Ok(rows) => rows,
            Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
        };

        courses.push(CourseRow {
            id: course.id,
            code: course.code,
            name: course.name,
            description: course.description,
            status: course.status,
            trimester: course.trimester,
            enrolled_count: course.enrolled_count,
            students,
        });
    }

    let mut ctx = base_ctx(&user, "courses");
    ctx.insert("total_courses", &courses.len());
    ctx.insert("courses", &courses);

    let rendered = match tmpl.render("lecturer/course.html", &ctx) {
        Ok(h) => h,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

// ─── CREATE WEEK ──────────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub struct CreateWeekForm {
    pub week_number: i32,
    pub title: String,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct AssignmentRow {
    pub id: i32,
    pub course_id: i32,
    pub course_code: String,
    pub course_name: String,
    pub week_number: Option<i32>,
    pub title: String,
    pub description: String,
    pub due_date: chrono::DateTime<chrono::Utc>,
    pub max_score: i32,
    pub file_count: Option<i64>,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct AssignmentFile {
    pub id: i32,
    pub file_name: String,
    pub file_path: String,
}

pub async fn create_week(
    cid: web::Path<i32>,
    form: web::Json<CreateWeekForm>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    let (_, lecturer_id) = match get_lecturer(&session, db.get_ref()).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let course_id = cid.into_inner();

    // Verify ownership
    let owns = sqlx::query!(
        "SELECT id FROM courses WHERE id = $1 AND lecturer_id = $2",
        course_id,
        lecturer_id
    )
    .fetch_optional(db.get_ref())
    .await;

    if !matches!(owns, Ok(Some(_))) {
        return HttpResponse::Forbidden().body("Not your course");
    }

    let result = sqlx::query!(
        "INSERT INTO course_weeks (course_id, week_number, title) VALUES ($1, $2, $3)",
        course_id,
        form.week_number,
        form.title
    )
    .execute(db.get_ref())
    .await;

    match result {
        Ok(_) => HttpResponse::Ok().json(json!({ "message": "Week created" })),
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
    }
}

// ─── UPLOAD FILE TO WEEK ──────────────────────────────────────────────────────

pub async fn upload_material(
    mut payload: actix_multipart::Multipart,
    db: web::Data<PgPool>,
    storage: web::Data<crate::storage::SupabaseStorage>,
    session: Session,
    path: web::Path<(i32, i32)>, // (course_id, week_id)
) -> impl Responder {
    use futures_util::TryStreamExt as _;

    // ✅ keep the user binding, don't prefix with _
    let user = match crate::auth::require_role(&session, crate::auth::UserRole::Lecturer) {
        Ok(u) => u,
        Err(r) => return r,
    };

    let (course_id, week_id) = path.into_inner();

    while let Ok(Some(mut field)) = payload.try_next().await {
        let cd = field.content_disposition().unwrap();
        let filename = cd.get_filename().unwrap_or("file").to_string();

        let mut bytes: Vec<u8> = Vec::new();
        while let Some(chunk) = field.try_next().await.unwrap() {
            bytes.extend_from_slice(&chunk);
        }

        let content_type = field
            .content_type()
            .map(|m| m.to_string())
            .unwrap_or_else(|| "application/pdf".to_string());

        let object_path = format!("courses/{course_id}/week_{week_id}/{filename}");

        if let Err(e) = storage.upload(&object_path, bytes, &content_type).await {
            return HttpResponse::InternalServerError().body(e);
        }

        if let Err(e) = sqlx::query!(
    "INSERT INTO course_materials (week_id, course_id, title, file_path, uploaded_by, material_type)
     VALUES ($1, $2, $3, $4, $5, $6)",
    week_id,
    course_id,
    filename,
    object_path,
    user.id,
    "pdf",        // or "lecture_note" etc.
)
        .execute(db.get_ref())
        .await
        {
            return HttpResponse::InternalServerError().body(e.to_string());
        }

        let actor = AuditActor {
            user_id: Some(user.id),
            role: Some("lecturer".to_string()),
            display_name: None,
        };
        log_audit_event(
            db.get_ref(),
            "content",
            "material_uploaded",
            "info",
            &actor,
            Some("course_material"),
            Some(course_id),
            Some(format!("Uploaded {filename} for week {week_id}")),
        )
        .await;
    }

    HttpResponse::SeeOther()
        .insert_header(("Location", "/lecturer/courses"))
        .finish()
}

// ─── DELETE WEEK ──────────────────────────────────────────────────────────────

pub async fn delete_week(
    path: web::Path<(i32, i32)>, // (course_id, week_id)
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    let (_, lecturer_id) = match get_lecturer(&session, db.get_ref()).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let (course_id, week_id) = path.into_inner();

    let valid = sqlx::query!(
        "SELECT cw.id FROM course_weeks cw
         JOIN courses c ON c.id = cw.course_id
         WHERE cw.id = $1 AND c.id = $2 AND c.lecturer_id = $3",
        week_id,
        course_id,
        lecturer_id
    )
    .fetch_optional(db.get_ref())
    .await;

    if !matches!(valid, Ok(Some(_))) {
        return HttpResponse::Forbidden().body("Invalid week or course");
    }

    // Delete files from disk
    let _ = std::fs::remove_dir_all(format!("uploads/courses/{}/week_{}", course_id, week_id));

    // DB cascade deletes course_materials with this week_id automatically
    let result = sqlx::query!("DELETE FROM course_weeks WHERE id = $1", week_id)
        .execute(db.get_ref())
        .await;

    match result {
        Ok(_) => HttpResponse::Ok().json(json!({ "message": "Week deleted" })),
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
    }
}

// ─── DELETE SINGLE FILE ───────────────────────────────────────────────────────

pub async fn delete_material(
    path: web::Path<i32>,
    db: web::Data<PgPool>,
    storage: web::Data<crate::storage::SupabaseStorage>,
    session: Session,
) -> impl Responder {
    let _user = match crate::auth::require_role(&session, crate::auth::UserRole::Lecturer) {
        Ok(u) => u,
        Err(r) => return r,
    };

    let material_id = path.into_inner();

    // Fetch the stored path before deleting from DB
    let material = match sqlx::query!(
        "SELECT file_path FROM course_materials WHERE id = $1",
        material_id
    )
    .fetch_optional(db.get_ref())
    .await
    {
        Ok(Some(m)) => m,
        Ok(None) => return HttpResponse::NotFound().body("Material not found"),
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };

    // Delete from Supabase Storage
    if let Err(e) = storage.delete(&material.file_path).await {
        return HttpResponse::InternalServerError().body(e);
    }

    // Delete from DB
    if let Err(e) = sqlx::query!("DELETE FROM course_materials WHERE id = $1", material_id)
        .execute(db.get_ref())
        .await
    {
        return HttpResponse::InternalServerError().body(e.to_string());
    }

    HttpResponse::Ok().finish()
}

// ─── OTHER PAGES (unchanged) ──────────────────────────────────────────────────

pub async fn lecturer_assignments_page(
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    let (user, lecturer_id) = match get_lecturer(&session, db.get_ref()).await {
        Ok(v) => v,
        Err(r) => return r,
    };

    #[derive(Serialize)]
    struct CourseRow {
        id: i32,
        code: String,
        name: String,
        description: String,
        status: String,
        trimester: String,
    }

    let courses = match sqlx::query_as!(
        CourseRow,
        r#"SELECT
            id,
            course_code                       AS code,
            course_name                       AS name,
            COALESCE(description, '')         AS "description!",
            COALESCE(status, 'Preparing')     AS "status!",
            COALESCE(trimester, '')           AS "trimester!"
           FROM courses
           WHERE lecturer_id = $1
           ORDER BY course_code"#,
        lecturer_id
    )
    .fetch_all(db.get_ref())
    .await
    {
        Ok(r) => r,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };

    let total_courses = courses.len();
    let ongoing_count = courses.iter().filter(|c| c.status == "Ongoing").count();
    let preparing_count = courses.iter().filter(|c| c.status == "Preparing").count();

    let mut ctx = base_ctx(&user, "assignments");
    ctx.insert("courses", &courses);
    ctx.insert("total_courses", &total_courses);
    ctx.insert("ongoing_count", &ongoing_count);
    ctx.insert("preparing_count", &preparing_count);

    let rendered = match tmpl.render("lecturer/assignments.html", &ctx) {
        Ok(h) => h,
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
        Ok(u) => u,
        Err(r) => return r,
    };
    let mut ctx = base_ctx(&user, "quizzes");

    let events = match sqlx::query_as::<_, crate::QuizMonitoringEventContext>(
        "SELECT id, quiz_id, student_display_name, event_type, severity, details,
         TO_CHAR(occurred_at AT TIME ZONE 'Asia/Singapore', 'YYYY-MM-DD HH24:MI:SS') AS occurred_at
         FROM quiz_monitoring_events ORDER BY occurred_at DESC LIMIT 50",
    )
    .fetch_all(db.get_ref())
    .await
    {
        Ok(e) => e,
        Err(error) => {
            ctx.insert(
                "monitoring_load_error",
                &format!("Could not load events: {error}"),
            );
            Vec::new()
        }
    };
    ctx.insert("monitoring_events", &events);
    ctx.insert("monitoring_event_count", &events.len());

    let rendered = match tmpl.render("lecturer/quizzes.html", &ctx) {
        Ok(h) => h,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

pub async fn lecturer_grades_page(
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    let (user, lecturer_id) = match get_lecturer(&session, db.get_ref()).await {
        Ok(v) => v,
        Err(r) => return r,
    };

    #[derive(Serialize)]
    struct LecturerGradeStudent {
        student_id: i32,
        student_name: String,
        email: String,
        assignment_score: f32,
        assignment_max: f32,
        assignment_percent: f32,
        quiz_score: f32,
        quiz_max: f32,
        quiz_percent: f32,
        overall_percent: f32,
        grade_letter: String,
        graded_assignment_count: i64,
        graded_quiz_count: i64,
    }

    #[derive(Serialize)]
    struct LecturerGradeCourse {
        id: i32,
        code: String,
        name: String,
        trimester: String,
        students: Vec<LecturerGradeStudent>,
        student_count: usize,
        course_average: f32,
        at_risk_count: usize,
    }

    #[derive(sqlx::FromRow)]
    struct GradebookRow {
        course_id: i32,
        course_code: String,
        course_name: String,
        trimester: String,
        student_id: i32,
        student_name: String,
        email: String,
        assignment_score: f64,
        assignment_max: f64,
        quiz_score: f64,
        quiz_max: f64,
        graded_assignment_count: i64,
        graded_quiz_count: i64,
    }

    let rows = match sqlx::query_as::<_, GradebookRow>(
        r#"WITH assignment_totals AS (
               SELECT
                   a.course_id,
                   s.student_id,
                   SUM(s.grade)::float8 AS assignment_score,
                   SUM(a.max_score)::float8 AS assignment_max,
                   COUNT(*)::BIGINT AS graded_assignment_count
               FROM submissions s
               JOIN assignments a ON a.id = s.assignment_id
               JOIN courses c ON c.id = a.course_id
               WHERE c.lecturer_id = $1
                 AND s.status = 'graded'
                 AND s.grade IS NOT NULL
               GROUP BY a.course_id, s.student_id
           ),
           quiz_best AS (
               SELECT
                   q.course_id,
                   qa.student_id,
                   q.id AS quiz_id,
                   MAX(qa.score)::float8 AS quiz_score,
                   q.total_marks::float8 AS quiz_max
               FROM quizzes q
               JOIN quiz_attempts qa ON qa.quiz_id = q.id
               JOIN courses c ON c.id = q.course_id
               WHERE c.lecturer_id = $1
                 AND q.is_practice = FALSE
                 AND qa.submitted_at IS NOT NULL
                 AND qa.score IS NOT NULL
               GROUP BY q.course_id, qa.student_id, q.id, q.total_marks
           ),
           quiz_totals AS (
               SELECT
                   course_id,
                   student_id,
                   SUM(quiz_score)::float8 AS quiz_score,
                   SUM(quiz_max)::float8 AS quiz_max,
                   COUNT(*)::BIGINT AS graded_quiz_count
               FROM quiz_best
               GROUP BY course_id, student_id
           )
           SELECT
               c.id AS course_id,
               c.course_code,
               c.course_name,
               COALESCE(c.trimester, '') AS trimester,
               st.id AS student_id,
               u.display_name AS student_name,
               u.email,
               COALESCE(at.assignment_score, 0)::float8 AS assignment_score,
               COALESCE(at.assignment_max, 0)::float8 AS assignment_max,
               COALESCE(qt.quiz_score, 0)::float8 AS quiz_score,
               COALESCE(qt.quiz_max, 0)::float8 AS quiz_max,
               COALESCE(at.graded_assignment_count, 0)::BIGINT AS graded_assignment_count,
               COALESCE(qt.graded_quiz_count, 0)::BIGINT AS graded_quiz_count
           FROM courses c
           JOIN enrollments e ON e.course_id = c.id
           JOIN students st ON st.id = e.student_id
           JOIN users u ON u.id = st.user_id
           LEFT JOIN assignment_totals at
             ON at.course_id = c.id AND at.student_id = st.id
           LEFT JOIN quiz_totals qt
             ON qt.course_id = c.id AND qt.student_id = st.id
           WHERE c.lecturer_id = $1
           ORDER BY c.course_code, u.display_name"#,
    )
    .bind(lecturer_id)
    .fetch_all(db.get_ref())
    .await
    {
        Ok(rows) => rows,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };

    let mut courses: Vec<LecturerGradeCourse> = Vec::new();
    for row in rows {
        let assignment_percent = percent(row.assignment_score, row.assignment_max);
        let quiz_percent = percent(row.quiz_score, row.quiz_max);
        let overall_percent = percent(
            row.assignment_score + row.quiz_score,
            row.assignment_max + row.quiz_max,
        );

        let student = LecturerGradeStudent {
            student_id: row.student_id,
            student_name: row.student_name,
            email: row.email,
            assignment_score: row.assignment_score as f32,
            assignment_max: row.assignment_max as f32,
            assignment_percent,
            quiz_score: row.quiz_score as f32,
            quiz_max: row.quiz_max as f32,
            quiz_percent,
            overall_percent,
            grade_letter: lecturer_grade_letter(overall_percent).to_string(),
            graded_assignment_count: row.graded_assignment_count,
            graded_quiz_count: row.graded_quiz_count,
        };

        if let Some(course) = courses.iter_mut().find(|course| course.id == row.course_id) {
            course.students.push(student);
        } else {
            courses.push(LecturerGradeCourse {
                id: row.course_id,
                code: row.course_code,
                name: row.course_name,
                trimester: row.trimester,
                students: vec![student],
                student_count: 0,
                course_average: 0.0,
                at_risk_count: 0,
            });
        }
    }

    for course in &mut courses {
        course.student_count = course.students.len();
        course.course_average = if course.students.is_empty() {
            0.0
        } else {
            course
                .students
                .iter()
                .map(|student| student.overall_percent)
                .sum::<f32>()
                / course.students.len() as f32
        };
        course.at_risk_count = course
            .students
            .iter()
            .filter(|student| {
                student.assignment_max + student.quiz_max > 0.0 && student.overall_percent < 60.0
            })
            .count();
    }

    let total_students: usize = courses.iter().map(|course| course.student_count).sum();
    let graded_students: usize = courses
        .iter()
        .flat_map(|course| &course.students)
        .filter(|student| student.assignment_max + student.quiz_max > 0.0)
        .count();
    let overall_average = if graded_students == 0 {
        0
    } else {
        (courses
            .iter()
            .flat_map(|course| &course.students)
            .filter(|student| student.assignment_max + student.quiz_max > 0.0)
            .map(|student| student.overall_percent)
            .sum::<f32>()
            / graded_students as f32)
            .round() as i32
    };

    let mut ctx = base_ctx(&user, "grades");
    ctx.insert("courses", &courses);
    ctx.insert("course_count", &courses.len());
    ctx.insert("student_count", &total_students);
    ctx.insert("graded_students", &graded_students);
    ctx.insert("overall_average", &overall_average);
    let rendered = match tmpl.render("lecturer/grades.html", &ctx) {
        Ok(h) => h,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

fn percent(score: f64, max_score: f64) -> f32 {
    if max_score > 0.0 {
        (score / max_score * 100.0) as f32
    } else {
        0.0
    }
}

fn lecturer_grade_letter(score: f32) -> &'static str {
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

pub async fn lecturer_profile_page(
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(u) => u,
        Err(r) => return r,
    };

    let profile = match sqlx::query_as::<_, LecturerProfileDetails>(
        "SELECT
             u.display_name,
             u.email,
             u.role,
             u.is_active,
             to_char(u.created_at AT TIME ZONE 'Asia/Singapore', 'YYYY-MM-DD HH24:MI:SS') AS created_at,
             l.id AS lecturer_id,
             l.staff_no,
             l.department,
             COUNT(c.id)::BIGINT AS assigned_courses
         FROM users u
         JOIN lecturers l ON l.user_id = u.id
         LEFT JOIN courses c ON c.lecturer_id = l.id
         WHERE u.id = $1
         GROUP BY u.id, l.id",
    )
    .bind(user.id)
    .fetch_optional(db.get_ref())
    .await
    {
        Ok(Some(profile)) => profile,
        Ok(None) => return HttpResponse::InternalServerError().body("Lecturer profile not found"),
        Err(error) => return HttpResponse::InternalServerError().body(error.to_string()),
    };

    let mut ctx = base_ctx(&user, "profile");
    ctx.insert("profile", &profile);
    let rendered = match tmpl.render("lecturer/profile.html", &ctx) {
        Ok(h) => h,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

pub async fn lecturer_settings_page(
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(u) => u,
        Err(r) => return r,
    };

    let preferences = match load_user_preferences(db.get_ref(), user.id).await {
        Ok(preferences) => preferences,
        Err(error) => return HttpResponse::InternalServerError().body(error.to_string()),
    };

    let mut ctx = base_ctx(&user, "settings");
    ctx.insert("preferences", &preferences);
    if let Ok(Some(message)) = session.get::<String>("settings_success") {
        ctx.insert("settings_success", &message);
        let _ = session.remove("settings_success");
    }

    let rendered = match tmpl.render("lecturer/settings.html", &ctx) {
        Ok(h) => h,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

pub async fn lecturer_settings_submit(
    db: web::Data<PgPool>,
    session: Session,
    form: web::Form<UserPreferencesForm>,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let form = form.into_inner();
    let theme_mode = if form.theme_mode == "dark" {
        "dark"
    } else {
        "light"
    };

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
        role: Some("lecturer".to_string()),
        display_name: None,
    };
    log_audit_event(
        db.get_ref(),
        "settings",
        "lecturer_settings_saved",
        "info",
        &actor,
        Some("preferences"),
        Some(user.id),
        Some(format!("Theme set to {theme_mode}")),
    )
    .await;

    let _ = session.insert("settings_success", "Settings saved.");
    let cookie_val = format!(
        "lms-theme={}; Path=/; Max-Age=31536000; SameSite=Lax",
        theme_mode
    );
    HttpResponse::SeeOther()
        .insert_header((actix_web::http::header::LOCATION, "/lecturer/settings"))
        .insert_header((actix_web::http::header::SET_COOKIE, cookie_val))
        .finish()
}

pub async fn lecturer_course_data(
    cid: web::Path<i32>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    let (_, lecturer_id) = match get_lecturer(&session, db.get_ref()).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let course_id = cid.into_inner();

    // Verify ownership
    let valid = sqlx::query!(
        "SELECT id FROM courses WHERE id = $1 AND lecturer_id = $2",
        course_id,
        lecturer_id
    )
    .fetch_optional(db.get_ref())
    .await;

    if !matches!(valid, Ok(Some(_))) {
        return HttpResponse::Forbidden().body("Not your course");
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
            .map(|f| json!({ "id": f.id, "title": f.title, "file_path": f.file_path }))
            .collect();

        weeks.push(json!({
            "id": w.id,
            "week_number": w.week_number,
            "title": w.title,
            "files": file_list
        }));
    }

    HttpResponse::Ok().json(json!({ "weeks": weeks }))
}

// ─── ASSIGNMENTS DATA (JSON) ──────────────────────────────────────────────────

pub async fn lecturer_assignments_data(
    db: web::Data<PgPool>,
    storage: web::Data<crate::storage::SupabaseStorage>,
    session: Session,
    query: web::Query<std::collections::HashMap<String, String>>,
) -> impl Responder {
    let (_, lecturer_id) = match get_lecturer(&session, db.get_ref()).await {
        Ok(v) => v,
        Err(r) => return r,
    };

    let course_id: i32 = match query.get("course_id").and_then(|v| v.parse().ok()) {
        Some(id) => id,
        None => return HttpResponse::BadRequest().body("Missing course_id"),
    };

    let assignments = match sqlx::query_as::<_, AssignmentRow>(
        r#"SELECT
            a.id,
            a.course_id,
            c.course_code,
            c.course_name,
            a.week_number,
            a.title,
            a.description,
            a.due_date,
            a.max_score,
            COUNT(af.id) AS file_count
           FROM assignments a
           JOIN courses c ON c.id = a.course_id
           LEFT JOIN assignment_files af ON af.assignment_id = a.id
           WHERE a.course_id = $1 AND c.lecturer_id = $2
           GROUP BY a.id, c.course_code, c.course_name
           ORDER BY a.week_number NULLS LAST, a.due_date"#,
    )
    .bind(course_id)
    .bind(lecturer_id)
    .fetch_all(db.get_ref())
    .await
    {
        Ok(r) => r,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };

    #[derive(Serialize)]
    struct AssignmentWithFiles {
        #[serde(flatten)]
        assignment: AssignmentRow,
        files: Vec<AssignmentFile>,
        submissions: Vec<serde_json::Value>,
        submission_count: usize,
    }

    #[derive(Serialize, sqlx::FromRow)]
    struct SubmissionRow {
        id: i32,
        student_name: String,
        file_path: String,
        submitted_at: chrono::DateTime<chrono::Utc>,
        status: String,
        grade: Option<f64>,
        feedback: Option<String>,
    }

    let mut result = Vec::new();
    for a in assignments {
        let files = sqlx::query_as!(
            AssignmentFile,
            "SELECT id, file_name, file_path FROM assignment_files WHERE assignment_id = $1",
            a.id
        )
        .fetch_all(db.get_ref())
        .await
        .unwrap_or_default();

        let submission_rows = sqlx::query_as::<_, SubmissionRow>(
            "SELECT
                 s.id,
                 u.display_name AS student_name,
                 s.file_path,
                 s.submitted_at,
                 s.status,
                 s.grade::float8 AS grade,
                 s.feedback
             FROM submissions s
             JOIN students st ON st.id = s.student_id
             JOIN users u ON u.id = st.user_id
             WHERE s.assignment_id = $1
             ORDER BY s.submitted_at DESC",
        )
        .bind(a.id)
        .fetch_all(db.get_ref())
        .await
        .unwrap_or_default();

        let submissions: Vec<serde_json::Value> = submission_rows
            .into_iter()
            .map(|s| {
                json!({
                    "id": s.id,
                    "student_name": s.student_name,
                    "file_name": storage_filename(&s.file_path),
                    "file_url": storage.public_url(&s.file_path),
                    "submitted_at": s.submitted_at,
                    "status": s.status,
                    "grade": s.grade,
                    "feedback": s.feedback,
                })
            })
            .collect();
        let submission_count = submissions.len();

        result.push(AssignmentWithFiles {
            assignment: a,
            files,
            submissions,
            submission_count,
        });
    }

    HttpResponse::Ok().json(result)
}

fn storage_filename(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

#[derive(serde::Deserialize)]
pub struct GradeSubmissionForm {
    grade: f64,
    feedback: Option<String>,
}

pub async fn grade_submission(
    db: web::Data<PgPool>,
    session: Session,
    path: web::Path<i32>,
    form: web::Json<GradeSubmissionForm>,
) -> impl Responder {
    let (_, lecturer_id) = match get_lecturer(&session, db.get_ref()).await {
        Ok(v) => v,
        Err(r) => return r,
    };

    let submission_id = path.into_inner();
    let feedback = form.feedback.as_deref().unwrap_or("").trim();
    let feedback = if feedback.is_empty() {
        None
    } else {
        Some(feedback.to_string())
    };

    if !form.grade.is_finite() || form.grade < 0.0 {
        return HttpResponse::BadRequest().body("Grade must be zero or higher");
    }

    let target = match sqlx::query(
        "SELECT a.max_score
         FROM submissions s
         JOIN assignments a ON a.id = s.assignment_id
         JOIN courses c ON c.id = a.course_id
         WHERE s.id = $1 AND c.lecturer_id = $2",
    )
    .bind(submission_id)
    .bind(lecturer_id)
    .fetch_optional(db.get_ref())
    .await
    {
        Ok(Some(row)) => row,
        Ok(None) => return HttpResponse::Forbidden().body("Submission not found for your course"),
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };

    use sqlx::Row;
    let max_score: i32 = target.get("max_score");
    if form.grade > max_score as f64 {
        return HttpResponse::BadRequest().body("Grade cannot exceed assignment max score");
    }

    let result = sqlx::query(
        "UPDATE submissions
         SET grade = $1::numeric, feedback = $2, status = 'graded'
         WHERE id = $3",
    )
    .bind(form.grade)
    .bind(feedback)
    .bind(submission_id)
    .execute(db.get_ref())
    .await;

    match result {
        Ok(_) => {
            let actor = AuditActor {
                user_id: Some(lecturer_id),
                role: Some("lecturer".to_string()),
                display_name: None,
            };
            log_audit_event(
                db.get_ref(),
                "course_management",
                "submission_graded",
                "warning",
                &actor,
                Some("submission"),
                Some(submission_id),
                Some(format!(
                    "Marked submission as graded with score {}",
                    form.grade
                )),
            )
            .await;
            HttpResponse::Ok().json(json!({ "ok": true }))
        }
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
    }
}

// ─── CREATE ASSIGNMENT ────────────────────────────────────────────────────────

pub async fn create_assignment(
    db: web::Data<PgPool>,
    storage: web::Data<crate::storage::SupabaseStorage>,
    session: Session,
    mut payload: Multipart,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(u) => u,
        Err(r) => return r,
    };

    let mut course_id: Option<i32> = None;
    let mut week_number: Option<i32> = None;
    let mut title = String::new();
    let mut description = String::new();
    let mut due_date: Option<String> = None;
    let mut max_score: Option<i32> = None;
    let mut pdf_files: Vec<(String, Vec<u8>)> = Vec::new();

    while let Some(Ok(mut field)) = payload.next().await {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "course_id" => {
                let mut buf = Vec::new();
                while let Some(Ok(c)) = field.next().await {
                    buf.extend_from_slice(&c);
                }
                course_id = String::from_utf8_lossy(&buf).parse().ok();
            }
            "week_number" => {
                let mut buf = Vec::new();
                while let Some(Ok(c)) = field.next().await {
                    buf.extend_from_slice(&c);
                }
                week_number = String::from_utf8_lossy(&buf).parse().ok();
            }
            "title" => {
                let mut buf = Vec::new();
                while let Some(Ok(c)) = field.next().await {
                    buf.extend_from_slice(&c);
                }
                title = String::from_utf8_lossy(&buf).to_string();
            }
            "description" => {
                let mut buf = Vec::new();
                while let Some(Ok(c)) = field.next().await {
                    buf.extend_from_slice(&c);
                }
                description = String::from_utf8_lossy(&buf).to_string();
            }
            "due_date" => {
                let mut buf = Vec::new();
                while let Some(Ok(c)) = field.next().await {
                    buf.extend_from_slice(&c);
                }
                due_date = Some(String::from_utf8_lossy(&buf).to_string());
            }
            "max_score" => {
                let mut buf = Vec::new();
                while let Some(Ok(c)) = field.next().await {
                    buf.extend_from_slice(&c);
                }
                max_score = String::from_utf8_lossy(&buf).parse().ok();
            }
            "files" => {
                let fname = field
                    .content_disposition()
                    .and_then(|cd| cd.get_filename())
                    .unwrap_or("file.pdf")
                    .to_string();
                let mut buf = Vec::new();
                while let Some(Ok(c)) = field.next().await {
                    buf.extend_from_slice(&c);
                }
                if !buf.is_empty() {
                    pdf_files.push((fname, buf));
                }
            }
            _ => while let Some(Ok(_)) = field.next().await {},
        }
    }

    let (course_id, week_number) = match (course_id, week_number) {
        (Some(c), Some(w)) => (c, w),
        _ => return HttpResponse::BadRequest().body("Missing course_id or week_number"),
    };

    let deadline_ts = match due_date.as_deref().and_then(|d| {
        chrono::NaiveDateTime::parse_from_str(d, "%Y-%m-%dT%H:%M")
            .ok()
            .map(|ndt| ndt.and_utc())
    }) {
        Some(t) => t,
        None => return HttpResponse::BadRequest().body("Invalid due_date format"),
    };

    let assignment = match sqlx::query!(
        r#"INSERT INTO assignments (course_id, week_number, title, description, due_date, max_score, created_by)
           VALUES ($1, $2, $3, $4, $5, $6, $7)
           RETURNING id"#,
        course_id,
        week_number,
        title,
        description,
        deadline_ts,
        max_score.unwrap_or(100),
        user.id,
    )
    .fetch_one(db.get_ref())
    .await {
        Ok(r) => r,
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };

    for (fname, bytes) in pdf_files {
        let object_path = format!("assignments/{}/{}/{}", course_id, assignment.id, fname);
        if let Err(e) = storage.upload(&object_path, bytes, "application/pdf").await {
            return HttpResponse::InternalServerError().body(e);
        }
        if let Err(e) = sqlx::query!(
            "INSERT INTO assignment_files (assignment_id, file_name, file_path) VALUES ($1, $2, $3)",
            assignment.id, fname, object_path,
        )
        .execute(db.get_ref())
        .await {
            return HttpResponse::InternalServerError().body(e.to_string());
        }
    }

    HttpResponse::Ok().json(json!({ "ok": true, "id": assignment.id }))
}

// ─── DELETE ASSIGNMENT ────────────────────────────────────────────────────────

pub async fn delete_assignment(
    db: web::Data<PgPool>,
    storage: web::Data<crate::storage::SupabaseStorage>,
    session: Session,
    path: web::Path<i32>,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let assignment_id = path.into_inner();

    let files = sqlx::query!(
        "SELECT file_path FROM assignment_files WHERE assignment_id = $1",
        assignment_id
    )
    .fetch_all(db.get_ref())
    .await
    .unwrap_or_default();

    for f in &files {
        let _ = storage.delete(&f.file_path).await;
    }

    let subs = sqlx::query!(
        "SELECT file_path FROM submissions WHERE assignment_id = $1",
        assignment_id
    )
    .fetch_all(db.get_ref())
    .await
    .unwrap_or_default();

    for s in &subs {
        let _ = storage.delete(&s.file_path).await;
    }

    match sqlx::query!("DELETE FROM assignments WHERE id = $1", assignment_id)
        .execute(db.get_ref())
        .await
    {
        Ok(_) => {
            let actor = AuditActor {
                user_id: Some(user.id),
                role: Some("lecturer".to_string()),
                display_name: Some(user.display_name.clone()),
            };
            log_audit_event(
                db.get_ref(),
                "course_management",
                "assignment_deleted",
                "warning",
                &actor,
                Some("assignment"),
                Some(assignment_id),
                Some("Deleted assignment".to_string()),
            )
            .await;
            HttpResponse::Ok().json(json!({ "ok": true }))
        }
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
    }
}
