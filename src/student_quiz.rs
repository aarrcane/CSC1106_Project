use actix_session::Session;
use actix_web::{HttpResponse, Responder, web};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use tera::{Context, Tera};

use crate::auth::UserRole;

// Fixed default quiz timing
const QUIZ_DURATION_MINS: i32 = 30;

// Attempts allowed is configured per-quiz (quizzes.attempts_allowed).

// DB Helpers

async fn student_id_for_user(db: &PgPool, user_id: i32) -> Result<Option<i32>, sqlx::Error> {
    sqlx::query_scalar::<_, i32>("SELECT id FROM students WHERE user_id = $1 LIMIT 1")
        .bind(user_id)
        .fetch_optional(db)
        .await
}

// True if the quiz exists AND the student is enrolled in its course.
async fn student_can_access(
    db: &PgPool,
    quiz_id: i32,
    student_id: i32,
) -> Result<bool, sqlx::Error> {
    let found: Option<i32> = sqlx::query_scalar(
        r#"SELECT q.id
             FROM quizzes q
             JOIN enrollments e ON e.course_id = q.course_id
            WHERE q.id = $1 AND e.student_id = $2"#,
    )
    .bind(quiz_id)
    .bind(student_id)
    .fetch_optional(db)
    .await?;
    Ok(found.is_some())
}

#[derive(FromRow)]
struct QuizGate {
    is_before_open: bool,
    is_after_close: bool,
}

async fn quiz_gate(db: &PgPool, quiz_id: i32) -> Result<Option<QuizGate>, sqlx::Error> {
    sqlx::query_as::<_, QuizGate>(
        "SELECT (NOW() < open_at) AS is_before_open, (NOW() > close_at) AS is_after_close FROM quizzes WHERE id = $1",
    )
    .bind(quiz_id)
    .fetch_optional(db)
    .await
}

async fn course_id_for_quiz(db: &PgPool, quiz_id: i32) -> Result<Option<i32>, sqlx::Error> {
    sqlx::query_scalar::<_, i32>("SELECT course_id FROM quizzes WHERE id = $1")
        .bind(quiz_id)
        .fetch_optional(db)
        .await
}

async fn attempts_used(db: &PgPool, quiz_id: i32, student_id: i32) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM quiz_attempts WHERE quiz_id = $1 AND student_id = $2 AND submitted_at IS NOT NULL",
    )
    .bind(quiz_id)
    .bind(student_id)
    .fetch_one(db)
    .await
}

async fn attempts_allowed(db: &PgPool, quiz_id: i32) -> Result<i32, sqlx::Error> {
    sqlx::query_scalar::<_, i32>("SELECT attempts_allowed FROM quizzes WHERE id = $1")
        .bind(quiz_id)
        .fetch_one(db)
        .await
}

// The in-progress (not yet submitted) attempt for this student+quiz, if any.
async fn in_progress_attempt(
    db: &PgPool,
    quiz_id: i32,
    student_id: i32,
) -> Result<Option<i32>, sqlx::Error> {
    sqlx::query_scalar::<_, i32>(
        "SELECT id FROM quiz_attempts WHERE quiz_id = $1 AND student_id = $2 AND submitted_at IS NULL ORDER BY id DESC LIMIT 1",
    )
    .bind(quiz_id)
    .bind(student_id)
    .fetch_optional(db)
    .await
}

// Start a new attempt: freeze a proficiency-targeted question subset; returns the attempt id, or None if no questions.
async fn create_attempt_with_subset(
    db: &PgPool,
    quiz_id: i32,
    student_id: i32,
) -> Result<Option<i32>, sqlx::Error> {
    let pool = crate::quiz_engine::load_questions(db, quiz_id).await?;
    if pool.is_empty() {
        return Ok(None);
    }

    let serve_cfg: Option<i32> =
        sqlx::query_scalar::<_, Option<i32>>("SELECT serve_count FROM quizzes WHERE id = $1")
            .bind(quiz_id)
            .fetch_one(db)
            .await?;
    let serve = serve_cfg.map(|n| n.max(1) as usize).unwrap_or(pool.len());

    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
        ^ (((student_id as u64) << 32) | quiz_id as u64);

    // Difficulty mix is fixed by pool + serve_count; the seed only varies which questions appear within each tier.
    let ids = crate::quiz_engine::select_balanced_subset_ids(&pool, serve, seed);

    let mut tx = db.begin().await?;
    let attempt_id: i32 = sqlx::query_scalar(
        "INSERT INTO quiz_attempts (quiz_id, student_id, started_at) VALUES ($1, $2, NOW()) RETURNING id",
    )
    .bind(quiz_id)
    .bind(student_id)
    .fetch_one(&mut *tx)
    .await?;
    for (pos, qid) in ids.iter().enumerate() {
        sqlx::query(
            "INSERT INTO quiz_attempt_questions (attempt_id, question_id, position) VALUES ($1, $2, $3)",
        )
        .bind(attempt_id)
        .bind(*qid)
        .bind(pos as i32)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(Some(attempt_id))
}

#[derive(FromRow)]
struct QuizMeta {
    title: String,
    course_code: String,
    course_name: String,
    total_marks: i32,
}

async fn quiz_meta(db: &PgPool, quiz_id: i32) -> Result<Option<QuizMeta>, sqlx::Error> {
    sqlx::query_as::<_, QuizMeta>(
        r#"SELECT q.title, c.course_code, c.course_name, q.total_marks
             FROM quizzes q JOIN courses c ON c.id = q.course_id WHERE q.id = $1"#,
    )
    .bind(quiz_id)
    .fetch_optional(db)
    .await
}

// GET /student/quizzes
pub async fn quiz_list(
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Student) {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let mut ctx = Context::new();

    let student_id = match student_id_for_user(db.get_ref(), user.id).await {
        Ok(Some(id)) => id,
        Ok(None) => {
            // No student record: render empty list rather than error.
            crate::insert_student_base(&mut ctx, &user.display_name, "");
            ctx.insert("active_page", "quizzes");
            ctx.insert("courses", &Vec::<crate::CourseContext>::new());
            ctx.insert("quizzes", &Vec::<crate::QuizContext>::new());
            ctx.insert("quiz_open_count", &0);
            ctx.insert("quiz_upcoming_count", &0);
            ctx.insert("quiz_completed_count", &0);
            ctx.insert("quiz_missed_count", &0);
            return render(&tmpl, "student/quiz.html", &ctx);
        }
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };

    #[derive(FromRow)]
    struct Row {
        id: i32,
        title: String,
        course_code: String,
        course_name: String,
        total_marks: i32,
        due_date: String,
        is_upcoming: bool,
        is_closed: bool,
        is_due_soon: bool,
        attempts_used: i64,
        last_score: Option<f32>,
        last_total: Option<i32>,
        attempts_allowed: i32,
    }
    let rows = sqlx::query_as::<_, Row>(
        r#"SELECT q.id, q.title, c.course_code, c.course_name, q.total_marks,
                  to_char(q.close_at, 'DD Mon YYYY') AS due_date,
                  (NOW() < q.open_at) AS is_upcoming,
                  (NOW() > q.close_at) AS is_closed,
                  (NOW() <= q.close_at AND q.close_at - NOW() < INTERVAL '3 days') AS is_due_soon,
                  (SELECT COUNT(*) FROM quiz_attempts a
                     WHERE a.quiz_id = q.id AND a.student_id = $1 AND a.submitted_at IS NOT NULL) AS attempts_used,
                  (SELECT a.score::float4 FROM quiz_attempts a
                     WHERE a.quiz_id = q.id AND a.student_id = $1 AND a.submitted_at IS NOT NULL
                     ORDER BY a.submitted_at DESC LIMIT 1) AS last_score,
                  (SELECT a.total_marks FROM quiz_attempts a
                     WHERE a.quiz_id = q.id AND a.student_id = $1 AND a.submitted_at IS NOT NULL
                     ORDER BY a.submitted_at DESC LIMIT 1) AS last_total,
                  q.attempts_allowed
             FROM quizzes q
             JOIN courses c ON c.id = q.course_id
             JOIN enrollments e ON e.course_id = c.id
            WHERE e.student_id = $1 AND q.is_practice = FALSE
            ORDER BY q.close_at"#,
    )
    .bind(student_id)
    .fetch_all(db.get_ref())
    .await
    .unwrap_or_default();

    let quizzes: Vec<crate::QuizContext> = rows
        .iter()
        .map(|r| {
            let completed = r.attempts_used > 0;
            let status = if completed {
                "completed"
            } else if r.is_upcoming {
                "upcoming"
            } else if r.is_closed {
                "missed"
            } else {
                "open"
            };
            let score = r.last_score.map(|s| {
                format!(
                    "{} / {}",
                    s.round() as i32,
                    r.last_total.unwrap_or(r.total_marks)
                )
            });
            crate::QuizContext {
                id: r.id,
                title: r.title.clone(),
                course_code: r.course_code.clone(),
                course_name: r.course_name.clone(),
                due_date: r.due_date.clone(),
                duration_mins: QUIZ_DURATION_MINS,
                status: status.into(),
                score,
                total_marks: r.total_marks,
                attempt_allowed: r.attempts_allowed,
                attempts_used: r.attempts_used as i32,
                urgent: status == "open" && r.is_due_soon,
            }
        })
        .collect();

    // Course filter list (distinct enrolled courses).
    #[derive(FromRow)]
    struct CourseRow {
        id: i32,
        course_code: String,
        course_name: String,
    }
    let course_rows = sqlx::query_as::<_, CourseRow>(
        r#"SELECT DISTINCT c.id, c.course_code, c.course_name
             FROM courses c JOIN enrollments e ON e.course_id = c.id
            WHERE e.student_id = $1 ORDER BY c.course_code"#,
    )
    .bind(student_id)
    .fetch_all(db.get_ref())
    .await
    .unwrap_or_default();
    let courses: Vec<crate::CourseContext> = course_rows
        .iter()
        .map(|c| crate::CourseContext {
            id: c.id,
            code: c.course_code.clone(),
            name: c.course_name.clone(),
            trimester: String::new(),
            image_url: String::new(),
            pinned: false,
            ongoing: true,
            progress: 0,
            lecturer: String::new(),
            attendance_pct: 0,
        })
        .collect();

    let open_count = quizzes.iter().filter(|q| q.status == "open").count();
    let upcoming_count = quizzes
        .iter()
        .filter(|q| q.status == "upcoming" || q.status == "open")
        .count();
    let completed_count = quizzes.iter().filter(|q| q.status == "completed").count();
    let missed_count = quizzes.iter().filter(|q| q.status == "missed").count();

    crate::insert_student_base(&mut ctx, &user.display_name, &student_id.to_string());
    ctx.insert("active_page", "quizzes");
    ctx.insert("courses", &courses);
    ctx.insert("quizzes", &quizzes);
    ctx.insert("quiz_open_count", &open_count);
    ctx.insert("quiz_upcoming_count", &upcoming_count);
    ctx.insert("quiz_completed_count", &completed_count);
    ctx.insert("quiz_missed_count", &missed_count);
    render(&tmpl, "student/quiz.html", &ctx)
}

// Monitoring Gate: Prevents student from guessing the url and going straight to the quiz page before the protoring check finishes
#[derive(Serialize)]
struct QuizHeader {
    id: i32,
    title: String,
    course_code: String,
    course_name: String,
    duration_mins: i32,
    total_marks: i32,
}

// GET /student/quizzes/{id}/attempt  (monitoring instruction gate)
pub async fn attempt_gate(
    path: web::Path<i32>,
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Student) {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let quiz_id = path.into_inner();
    let student_id = match student_id_for_user(db.get_ref(), user.id).await {
        Ok(Some(id)) => id,
        Ok(None) => return HttpResponse::Forbidden().body("No student record for this user."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };
    match student_can_access(db.get_ref(), quiz_id, student_id).await {
        Ok(true) => {}
        Ok(false) => return HttpResponse::NotFound().body("Quiz not available."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    }
    let meta = match quiz_meta(db.get_ref(), quiz_id).await {
        Ok(Some(m)) => m,
        Ok(None) => return HttpResponse::NotFound().body("Quiz not found."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };

    // Reset the monitoring gate for this quiz.
    if let Err(e) = session.insert(crate::quiz_monitoring_ready_key(quiz_id), false) {
        return HttpResponse::InternalServerError().body(format!("Session error: {e}"));
    }

    let quiz = QuizHeader {
        id: quiz_id,
        title: meta.title,
        course_code: meta.course_code,
        course_name: meta.course_name,
        duration_mins: QUIZ_DURATION_MINS,
        total_marks: meta.total_marks,
    };
    let mut ctx = Context::new();
    crate::insert_student_base(&mut ctx, &user.display_name, &student_id.to_string());
    ctx.insert("active_page", "quizzes");
    ctx.insert("quiz", &quiz);
    ctx.insert(
        "monitoring_event_url",
        &format!("/student/quizzes/{quiz_id}/monitoring-events"),
    );
    ctx.insert(
        "monitoring_ready_url",
        &format!("/student/quizzes/{quiz_id}/monitoring-ready"),
    );
    ctx.insert(
        "quiz_start_url",
        &format!("/student/quizzes/{quiz_id}/take"),
    );
    render(&tmpl, "student/quiz_attempt.html", &ctx)
}

#[derive(Serialize)]
struct TakeOption {
    id: i32,
    option_text: String,
}
#[derive(Serialize)]
struct TakeQuestion {
    id: i32,
    number: i32,
    question_text: String,
    question_type: String,
    options: Vec<TakeOption>,
}

// GET /student/quizzes/{id}/take
pub async fn take(
    path: web::Path<i32>,
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Student) {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let quiz_id = path.into_inner();
    let student_id = match student_id_for_user(db.get_ref(), user.id).await {
        Ok(Some(id)) => id,
        Ok(None) => return HttpResponse::Forbidden().body("No student record for this user."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };
    match student_can_access(db.get_ref(), quiz_id, student_id).await {
        Ok(true) => {}
        Ok(false) => return HttpResponse::NotFound().body("Quiz not available."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    }

    // Must pass the monitoring gate first.
    match crate::quiz_monitoring_ready(&session, quiz_id) {
        Ok(true) => {}
        Ok(false) => {
            return HttpResponse::SeeOther()
                .insert_header(("Location", format!("/student/quizzes/{quiz_id}/attempt")))
                .finish();
        }
        Err(resp) => return resp,
    }

    // Resume an in-progress attempt or start a new one; the question subset is frozen at creation.
    let attempt_id = match in_progress_attempt(db.get_ref(), quiz_id, student_id).await {
        Ok(Some(id)) => id,
        Ok(None) => {
            let used = match attempts_used(db.get_ref(), quiz_id, student_id).await {
                Ok(n) => n,
                Err(e) => {
                    return HttpResponse::InternalServerError().body(format!("DB error: {e}"));
                }
            };
            let allowed = match attempts_allowed(db.get_ref(), quiz_id).await {
                Ok(n) => n,
                Err(e) => {
                    return HttpResponse::InternalServerError().body(format!("DB error: {e}"));
                }
            };
            if used >= allowed as i64 {
                return HttpResponse::SeeOther()
                    .insert_header(("Location", format!("/student/quizzes/{quiz_id}/result")))
                    .finish();
            }
            match create_attempt_with_subset(db.get_ref(), quiz_id, student_id).await {
                Ok(Some(id)) => id,
                Ok(None) => {
                    return HttpResponse::BadRequest().body("This quiz has no questions yet.");
                }
                Err(e) => {
                    return HttpResponse::InternalServerError().body(format!("DB error: {e}"));
                }
            }
        }
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };

    let meta = match quiz_meta(db.get_ref(), quiz_id).await {
        Ok(Some(m)) => m,
        Ok(None) => return HttpResponse::NotFound().body("Quiz not found."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };

    // Load the served questions for this attempt (student-facing: no is_correct).
    let served = match crate::quiz_engine::load_served_questions(db.get_ref(), attempt_id).await {
        Ok(q) => q,
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };
    if served.is_empty() {
        return HttpResponse::BadRequest().body("This quiz has no questions yet.");
    }
    let served_total: i32 = served.iter().map(|q| q.marks).sum();
    let questions: Vec<TakeQuestion> = served
        .iter()
        .enumerate()
        .map(|(i, q)| TakeQuestion {
            id: q.id,
            number: (i + 1) as i32,
            question_text: q.prompt.clone(),
            question_type: q.question_type.clone(),
            options: q
                .options
                .iter()
                .map(|o| TakeOption {
                    id: o.id,
                    option_text: o.option_text.clone(),
                })
                .collect(),
        })
        .collect();

    let quiz = QuizHeader {
        id: quiz_id,
        title: meta.title,
        course_code: meta.course_code,
        course_name: meta.course_name,
        duration_mins: QUIZ_DURATION_MINS,
        total_marks: served_total,
    };
    let mut ctx = Context::new();
    crate::insert_student_base(&mut ctx, &user.display_name, &student_id.to_string());
    ctx.insert("active_page", "quizzes");
    ctx.insert("quiz", &quiz);
    ctx.insert("questions", &questions);
    ctx.insert("quiz_seconds", &(QUIZ_DURATION_MINS * 60));
    ctx.insert("submit_url", &format!("/student/quizzes/{quiz_id}/submit"));
    ctx.insert(
        "monitoring_event_url",
        &format!("/student/quizzes/{quiz_id}/monitoring-events"),
    );
    ctx.insert(
        "monitoring_ready_url",
        &format!("/student/quizzes/{quiz_id}/monitoring-ready"),
    );
    render(&tmpl, "student/quiz_take.html", &ctx)
}

// Student being granted entry to the quiz

// POST /student/quizzes/{id}/monitoring-ready
pub async fn mark_monitoring_ready(
    path: web::Path<i32>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Student) {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let quiz_id = path.into_inner();
    let student_id = match student_id_for_user(db.get_ref(), user.id).await {
        Ok(Some(id)) => id,
        Ok(None) => return HttpResponse::Forbidden().body("No student record for this user."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };
    match student_can_access(db.get_ref(), quiz_id, student_id).await {
        Ok(true) => {}
        Ok(false) => return HttpResponse::NotFound().body("Quiz not available."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    }
    match session.insert(crate::quiz_monitoring_ready_key(quiz_id), true) {
        Ok(_) => HttpResponse::Ok().json(crate::QuizMonitoringEventResponse { status: "ready" }),
        Err(e) => HttpResponse::InternalServerError().body(format!("Session error: {e}")),
    }
}

// POST /student/quizzes/{id}/monitoring-events
pub async fn save_monitoring_event(
    path: web::Path<i32>,
    db: web::Data<PgPool>,
    session: Session,
    payload: web::Json<crate::QuizMonitoringEventPayload>,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Student) {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let quiz_id = path.into_inner();
    let student_id = match student_id_for_user(db.get_ref(), user.id).await {
        Ok(Some(id)) => id,
        Ok(None) => return HttpResponse::Forbidden().body("No student record for this user."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };
    match student_can_access(db.get_ref(), quiz_id, student_id).await {
        Ok(true) => {}
        Ok(false) => return HttpResponse::NotFound().body("Quiz not available."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    }

    let event_type = payload.event_type.trim().to_lowercase();
    let severity = payload.severity.trim().to_lowercase();
    if !crate::valid_monitoring_event_type(&event_type) {
        return HttpResponse::BadRequest().body("Unknown monitoring event type.");
    }
    if !crate::valid_monitoring_severity(&severity) {
        return HttpResponse::BadRequest().body("Unknown monitoring event severity.");
    }
    let details = crate::truncate_details(payload.details.as_deref());
    let result = sqlx::query(
        "INSERT INTO quiz_monitoring_events
            (quiz_id, student_user_id, student_display_name, event_type, severity, details)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(quiz_id)
    .bind(user.id)
    .bind(&user.display_name)
    .bind(&event_type)
    .bind(&severity)
    .bind(details)
    .execute(db.get_ref())
    .await;
    match result {
        Ok(_) => HttpResponse::Ok().json(crate::QuizMonitoringEventResponse { status: "saved" }),
        Err(e) => HttpResponse::InternalServerError().body(format!("Failed to save event: {e}")),
    }
}

// Submit + Grade
#[derive(Deserialize)]
pub struct AnswerInput {
    pub question_id: i32,
    pub selected_option_id: Option<i32>,
}
#[derive(Deserialize)]
pub struct SubmitPayload {
    pub answers: Vec<AnswerInput>,
}

#[derive(Serialize)]
struct SubmitResult {
    ok: bool,
    message: String,
    redirect: Option<String>,
}

// POST /student/quizzes/{id}/submit  (JSON body)
pub async fn submit(
    path: web::Path<i32>,
    db: web::Data<PgPool>,
    session: Session,
    payload: web::Json<SubmitPayload>,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Student) {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let quiz_id = path.into_inner();
    let student_id = match student_id_for_user(db.get_ref(), user.id).await {
        Ok(Some(id)) => id,
        Ok(None) => return HttpResponse::Forbidden().body("No student record for this user."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };
    match student_can_access(db.get_ref(), quiz_id, student_id).await {
        Ok(true) => {}
        Ok(false) => return HttpResponse::NotFound().body("Quiz not available."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    }

    // Enforce attempt window.
    match quiz_gate(db.get_ref(), quiz_id).await {
        Ok(Some(g)) if g.is_before_open => return submit_err("This quiz is not open yet."),
        Ok(Some(g)) if g.is_after_close => return submit_err("This quiz has closed."),
        Ok(Some(_)) => {}
        Ok(None) => return HttpResponse::NotFound().body("Quiz not found."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    }
    // Finalize the in-progress attempt that was created when the quiz started.
    let attempt_id = match in_progress_attempt(db.get_ref(), quiz_id, student_id).await {
        Ok(Some(id)) => id,
        Ok(None) => return submit_err("No active attempt - start the quiz first."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };

    // Resolve the course (needed for topic-proficiency tracking).
    let course_id = match course_id_for_quiz(db.get_ref(), quiz_id).await {
        Ok(Some(id)) => id,
        Ok(None) => return HttpResponse::NotFound().body("Quiz not found."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };

    // Grade only the questions that were served for this attempt.
    let questions = match crate::quiz_engine::load_served_questions(db.get_ref(), attempt_id).await
    {
        Ok(q) if !q.is_empty() => q,
        Ok(_) => return submit_err("This attempt has no questions."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };

    // Grade with the shared engine.
    let submission = crate::quiz_engine::AttemptSubmission {
        answers: payload
            .answers
            .iter()
            .map(|a| crate::quiz_engine::AnswerSubmission {
                question_id: a.question_id,
                selected_option_id: a.selected_option_id,
            })
            .collect(),
    };
    let result = crate::quiz_engine::grade_attempt(&questions, &submission);

    // Finalize attempt + answers AND update topic proficiency (single transaction).
    if let Err(e) = crate::quiz_engine::persist_attempt(
        db.get_ref(),
        attempt_id,
        student_id,
        course_id,
        &submission,
        &result,
    )
    .await
    {
        return HttpResponse::InternalServerError().body(format!("Save failed: {e}"));
    }

    // Clear the monitoring gate so a refresh of /take won't re-enter.
    let _ = session.insert(crate::quiz_monitoring_ready_key(quiz_id), false);

    HttpResponse::Ok().json(SubmitResult {
        ok: true,
        message: "Submitted.".into(),
        redirect: Some(format!("/student/quizzes/{quiz_id}/result")),
    })
}

fn submit_err(msg: &str) -> HttpResponse {
    HttpResponse::BadRequest().json(SubmitResult {
        ok: false,
        message: msg.into(),
        redirect: None,
    })
}

// Result
#[derive(Serialize)]
struct ResultQuestion {
    number: i32,
    question_text: String,
    your_answer: String,
    correct_answer: String,
    is_correct: bool,
    marks: i32,
    marks_awarded: i32,
}

// GET /student/quizzes/{id}/result
pub async fn result(
    path: web::Path<i32>,
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Student) {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let quiz_id = path.into_inner();
    let student_id = match student_id_for_user(db.get_ref(), user.id).await {
        Ok(Some(id)) => id,
        Ok(None) => return HttpResponse::Forbidden().body("No student record for this user."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };
    match student_can_access(db.get_ref(), quiz_id, student_id).await {
        Ok(true) => {}
        Ok(false) => return HttpResponse::NotFound().body("Quiz not available."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    }
    let meta = match quiz_meta(db.get_ref(), quiz_id).await {
        Ok(Some(m)) => m,
        Ok(None) => return HttpResponse::NotFound().body("Quiz not found."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };

    // Latest submitted attempt.
    #[derive(FromRow)]
    struct AttemptRow {
        id: i32,
        score: Option<f32>,
        total_marks: Option<i32>,
        submitted_at: Option<String>,
    }
    let attempt = sqlx::query_as::<_, AttemptRow>(
        r#"SELECT id, score::float4 AS score, total_marks, to_char(submitted_at, 'DD Mon YYYY, HH24:MI') AS submitted_at
             FROM quiz_attempts
            WHERE quiz_id = $1 AND student_id = $2 AND submitted_at IS NOT NULL
            ORDER BY submitted_at DESC LIMIT 1"#,
    )
    .bind(quiz_id)
    .bind(student_id)
    .fetch_optional(db.get_ref())
    .await;

    let attempt = match attempt {
        Ok(Some(a)) => a,
        Ok(None) => {
            // No attempt yet -> send them to the gate.
            return HttpResponse::SeeOther()
                .insert_header(("Location", format!("/student/quizzes/{quiz_id}/attempt")))
                .finish();
        }
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };

    // Per-question breakdown for that attempt.
    #[derive(FromRow)]
    struct BreakRow {
        question_text: String,
        marks: i32,
        is_correct: bool,
        your_answer: Option<String>,
        correct_answer: Option<String>,
    }
    let rows = sqlx::query_as::<_, BreakRow>(
        r#"SELECT qq.question_text,
                  qq.marks,
                  ans.is_correct,
                  chosen.option_text AS your_answer,
                  correct.option_text AS correct_answer
             FROM quiz_attempt_questions aq
             JOIN quiz_questions qq ON qq.id = aq.question_id
             LEFT JOIN quiz_answers ans
                    ON ans.question_id = qq.id AND ans.attempt_id = $1
             LEFT JOIN quiz_options chosen
                    ON chosen.id = ans.selected_option_id
             LEFT JOIN LATERAL (
                    SELECT option_text FROM quiz_options o
                     WHERE o.question_id = qq.id AND o.is_correct = TRUE
                     ORDER BY o.id LIMIT 1
                  ) correct ON TRUE
            WHERE aq.attempt_id = $1
            ORDER BY aq.position"#,
    )
    .bind(attempt.id)
    .fetch_all(db.get_ref())
    .await
    .unwrap_or_default();

    let mut questions = Vec::with_capacity(rows.len());
    for (i, r) in rows.iter().enumerate() {
        questions.push(ResultQuestion {
            number: (i + 1) as i32,
            question_text: r.question_text.clone(),
            your_answer: r.your_answer.clone().unwrap_or_else(|| "No answer".into()),
            correct_answer: r.correct_answer.clone().unwrap_or_default(),
            is_correct: r.is_correct,
            marks: r.marks,
            marks_awarded: if r.is_correct { r.marks } else { 0 },
        });
    }

    let total = attempt.total_marks.unwrap_or(meta.total_marks);
    let score = attempt.score.unwrap_or(0.0).round() as i32;
    let percentage = if total > 0 {
        (score as f32 / total as f32 * 100.0).round() as i32
    } else {
        0
    };

    // Competency readout (per-topic proficiency) + recommended materials.
    let course_id = course_id_for_quiz(db.get_ref(), quiz_id)
        .await
        .ok()
        .flatten()
        .unwrap_or(0);

    #[derive(FromRow)]
    struct ProfRow {
        topic: String,
        proficiency: f32,
    }
    let prof_rows = sqlx::query_as::<_, ProfRow>(
        "SELECT topic, proficiency::float4 AS proficiency FROM student_topic_proficiency WHERE student_id = $1 AND course_id = $2 ORDER BY topic",
    )
    .bind(student_id)
    .bind(course_id)
    .fetch_all(db.get_ref())
    .await
    .unwrap_or_default();

    #[derive(Serialize)]
    struct Competency {
        topic: String,
        percent: i32,
        level: String,
    }
    let competencies: Vec<Competency> = prof_rows
        .iter()
        .map(|r| {
            let level = if r.proficiency >= 0.75 {
                "Advanced"
            } else if r.proficiency >= 0.45 {
                "Intermediate"
            } else {
                "Beginner"
            };
            Competency {
                topic: r.topic.clone(),
                percent: (r.proficiency * 100.0).round() as i32,
                level: level.into(),
            }
        })
        .collect();

    let recommendations =
        crate::quiz_engine::recommend_materials(db.get_ref(), student_id, course_id, 0.6, 5)
            .await
            .unwrap_or_default();

    let mut ctx = Context::new();
    crate::insert_student_base(&mut ctx, &user.display_name, &student_id.to_string());
    ctx.insert("active_page", "quizzes");
    ctx.insert("quiz_id", &quiz_id);
    ctx.insert("quiz_title", &meta.title);
    ctx.insert("course_code", &meta.course_code);
    ctx.insert("score", &score);
    ctx.insert("total_marks", &total);
    ctx.insert("percentage", &percentage);
    ctx.insert("submitted_at", &attempt.submitted_at.unwrap_or_default());
    ctx.insert("questions", &questions);
    ctx.insert("competencies", &competencies);
    ctx.insert("recommendations", &recommendations);
    render(&tmpl, "student/quiz_result.html", &ctx)
}

fn render(tmpl: &Tera, name: &str, ctx: &Context) -> HttpResponse {
    match tmpl.render(name, ctx) {
        Ok(html) => HttpResponse::Ok().content_type("text/html").body(html),
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
    }
}
