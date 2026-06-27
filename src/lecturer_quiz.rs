use actix_session::Session;
use actix_web::{web, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use tera::{Context, Tera};

use crate::auth::UserRole;

fn default_difficulty() -> i16 { 1 }
fn default_attempts() -> i32 { 1 }

#[derive(Debug, Clone, Deserialize)]
pub struct OptionInput {
    pub option_text: String,
    pub is_correct: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QuestionInput {
    pub question_text: String,
    pub question_type: String, // 'multiple_choice' | 'true_false'
    pub marks: i32,
    #[serde(default = "default_difficulty")]
    pub difficulty: i16, // 1..5
    #[serde(default)]
    pub topic: Option<String>,
    pub options: Vec<OptionInput>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QuizInput {
    pub course_id: i32,
    pub title: String,
    pub description: Option<String>,
    /// datetime-local strings e.g. "2026-06-20T10:00" (cast to TIMESTAMPTZ in SQL).
    pub open_at: String,
    pub close_at: String,
    #[serde(default)]
    pub serve_count: Option<i32>, // NULL = serve all
    #[serde(default = "default_attempts")]
    pub attempts_allowed: i32,
    pub questions: Vec<QuestionInput>,
}

// Validate payload; returns total marks on success or an error message.
fn validate(input: &QuizInput) -> Result<i32, String> {
    if input.title.trim().is_empty() {
        return Err("Quiz title is required.".into());
    }
    if input.open_at.trim().is_empty() || input.close_at.trim().is_empty() {
        return Err("Open and close times are required.".into());
    }
    if input.questions.is_empty() {
        return Err("Add at least one question.".into());
    }
    let mut total = 0;
    for (i, q) in input.questions.iter().enumerate() {
        let n = i + 1;
        if q.question_text.trim().is_empty() {
            return Err(format!("Question {n}: text is required."));
        }
        if q.question_type != "multiple_choice" && q.question_type != "true_false" {
            return Err(format!("Question {n}: invalid type."));
        }
        if q.marks <= 0 {
            return Err(format!("Question {n}: marks must be greater than 0."));
        }
        if q.difficulty < 1 || q.difficulty > 5 {
            return Err(format!("Question {n}: difficulty must be between 1 and 5."));
        }
        if q.options.len() < 2 {
            return Err(format!("Question {n}: needs at least two options."));
        }
        if q.options.iter().filter(|o| o.is_correct).count() != 1 {
            return Err(format!("Question {n}: mark exactly one option correct."));
        }
        if q.options.iter().any(|o| o.option_text.trim().is_empty()) {
            return Err(format!("Question {n}: every option needs text."));
        }
        total += q.marks;
    }
    if input.attempts_allowed < 1 {
        return Err("Attempts allowed must be at least 1.".into());
    }
    if let Some(sc) = input.serve_count {
        if sc < 1 || (sc as usize) > input.questions.len() {
            return Err("Questions to serve must be between 1 and the number of questions.".into());
        }
    }
    Ok(total)
}


// Display Structs

#[derive(Debug, Serialize, FromRow)]
struct LecturerCourse {
    id: i32,
    course_code: String,
    course_name: String,
}

#[derive(Debug, Serialize, FromRow)]
struct QuizListRow {
    id: i32,
    title: String,
    course_code: String,
    open_at: String,
    close_at: String,
    total_marks: i32,
    question_count: i64,
    attempt_count: i64,
    status: String,
}

#[derive(Debug, Serialize, FromRow)]
struct AttemptRow {
    student_name: String,
    student_email: String,
    submitted_at: Option<String>,
    score: Option<f32>,
    percentage: Option<f32>,
}

// Edit-prefill structs (serialised to JSON for the builder via Tera json_encode).
#[derive(Debug, Serialize)]
struct EditOption {
    option_text: String,
    is_correct: bool,
}
#[derive(Debug, Serialize)]
struct EditQuestion {
    question_text: String,
    question_type: String,
    marks: i32,
    difficulty: i16,
    topic: String,
    options: Vec<EditOption>,
}
#[derive(Debug, Serialize)]
struct EditQuiz {
    id: i32,
    course_id: i32,
    title: String,
    description: String,
    open_at: String,
    close_at: String,
    serve_count: Option<i32>,
    attempts_allowed: i32,
    questions: Vec<EditQuestion>,
}

#[derive(Serialize)]
struct ApiResult {
    ok: bool,
    message: String,
    redirect: Option<String>,
}

// DB helpers

async fn lecturer_id_for_user(db: &PgPool, user_id: i32) -> Result<Option<i32>, sqlx::Error> {
    sqlx::query_scalar::<_, i32>("SELECT id FROM lecturers WHERE user_id = $1 LIMIT 1")
        .bind(user_id)
        .fetch_optional(db)
        .await
}

async fn course_owned(db: &PgPool, course_id: i32, lecturer_id: i32) -> Result<bool, sqlx::Error> {
    let found: Option<i32> =
        sqlx::query_scalar("SELECT id FROM courses WHERE id = $1 AND lecturer_id = $2")
            .bind(course_id)
            .bind(lecturer_id)
            .fetch_optional(db)
            .await?;
    Ok(found.is_some())
}

async fn quiz_owned(db: &PgPool, quiz_id: i32, lecturer_id: i32) -> Result<bool, sqlx::Error> {
    let found: Option<i32> = sqlx::query_scalar(
        r#"SELECT q.id FROM quizzes q
             JOIN courses c ON c.id = q.course_id
            WHERE q.id = $1 AND c.lecturer_id = $2"#,
    )
    .bind(quiz_id)
    .bind(lecturer_id)
    .fetch_optional(db)
    .await?;
    Ok(found.is_some())
}

async fn lecturer_courses(db: &PgPool, lecturer_id: i32) -> Result<Vec<LecturerCourse>, sqlx::Error> {
    sqlx::query_as::<_, LecturerCourse>(
        "SELECT id, course_code, course_name FROM courses WHERE lecturer_id = $1 ORDER BY course_code",
    )
    .bind(lecturer_id)
    .fetch_all(db)
    .await
}

async fn insert_questions(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    quiz_id: i32,
    questions: &[QuestionInput],
) -> Result<(), sqlx::Error> {
    for q in questions {
        let question_id: i32 = sqlx::query_scalar(
            r#"INSERT INTO quiz_questions (quiz_id, question_text, question_type, marks, difficulty, topic)
               VALUES ($1, $2, $3, $4, $5, $6) RETURNING id"#,
        )
        .bind(quiz_id)
        .bind(q.question_text.trim())
        .bind(&q.question_type)
        .bind(q.marks)
        .bind(q.difficulty)
        .bind(q.topic.as_deref().map(|t| t.trim()).filter(|t| !t.is_empty()))
        .fetch_one(&mut **tx)
        .await?;

        for o in &q.options {
            sqlx::query(
                "INSERT INTO quiz_options (question_id, option_text, is_correct) VALUES ($1, $2, $3)",
            )
            .bind(question_id)
            .bind(o.option_text.trim())
            .bind(o.is_correct)
            .execute(&mut **tx)
            .await?;
        }
    }
    Ok(())
}

async fn insert_quiz(
    db: &PgPool,
    created_by: i32,
    total_marks: i32,
    input: &QuizInput,
) -> Result<i32, sqlx::Error> {
    let mut tx = db.begin().await?;
    let quiz_id: i32 = sqlx::query_scalar(
        r#"INSERT INTO quizzes
               (course_id, created_by, title, description, open_at, close_at, total_marks, serve_count, attempts_allowed)
           VALUES ($1, $2, $3, $4, CAST($5 AS TIMESTAMPTZ), CAST($6 AS TIMESTAMPTZ), $7, $8, $9)
           RETURNING id"#,
    )
    .bind(input.course_id)
    .bind(created_by)
    .bind(input.title.trim())
    .bind(input.description.as_deref())
    .bind(&input.open_at)
    .bind(&input.close_at)
    .bind(total_marks)
    .bind(input.serve_count)
    .bind(input.attempts_allowed)
    .fetch_one(&mut *tx)
    .await?;

    insert_questions(&mut tx, quiz_id, &input.questions).await?;
    tx.commit().await?;
    Ok(quiz_id)
}

async fn update_quiz(
    db: &PgPool,
    quiz_id: i32,
    total_marks: i32,
    input: &QuizInput,
) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;
    sqlx::query(
        r#"UPDATE quizzes
              SET course_id = $2, title = $3, description = $4,
                  open_at = CAST($5 AS TIMESTAMPTZ), close_at = CAST($6 AS TIMESTAMPTZ),
                  total_marks = $7, serve_count = $8, attempts_allowed = $9
            WHERE id = $1"#,
    )
    .bind(quiz_id)
    .bind(input.course_id)
    .bind(input.title.trim())
    .bind(input.description.as_deref())
    .bind(&input.open_at)
    .bind(&input.close_at)
    .bind(total_marks)
    .bind(input.serve_count)
    .bind(input.attempts_allowed)
    .execute(&mut *tx)
    .await?;

    sqlx::query("DELETE FROM quiz_questions WHERE quiz_id = $1")
        .bind(quiz_id)
        .execute(&mut *tx)
        .await?;
    insert_questions(&mut tx, quiz_id, &input.questions).await?;
    tx.commit().await?;
    Ok(())
}


// Handlers
fn base_ctx(ctx: &mut Context, display_name: &str) {
    ctx.insert("display_name", display_name);
    ctx.insert("student_name", display_name);
    ctx.insert("student_id", "");
    ctx.insert("notifications", &Vec::<crate::NotificationContext>::new());
    ctx.insert("active_page", "quiz_manage");
    ctx.insert("is_lecturer", &true);
}

fn render(tmpl: &Tera, name: &str, ctx: &Context) -> HttpResponse {
    match tmpl.render(name, ctx) {
        Ok(html) => HttpResponse::Ok().content_type("text/html").body(html),
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
    }
}

fn json_err(msg: &str) -> HttpResponse {
    HttpResponse::BadRequest().json(ApiResult { ok: false, message: msg.into(), redirect: None })
}

// GET /lecturer/quizzes/manage
pub async fn manage_list(
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let lecturer_id = match lecturer_id_for_user(db.get_ref(), user.id).await {
        Ok(Some(id)) => id,
        Ok(None) => return HttpResponse::Forbidden().body("No lecturer record for this user."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };

    let quizzes = sqlx::query_as::<_, QuizListRow>(
        r#"SELECT q.id, q.title, c.course_code,
                  to_char(q.open_at,  'DD Mon YYYY, HH24:MI') AS open_at,
                  to_char(q.close_at, 'DD Mon YYYY, HH24:MI') AS close_at,
                  q.total_marks,
                  (SELECT COUNT(*) FROM quiz_questions qq WHERE qq.quiz_id = q.id) AS question_count,
                  (SELECT COUNT(*) FROM quiz_attempts  qa WHERE qa.quiz_id = q.id AND qa.submitted_at IS NOT NULL) AS attempt_count,
                  CASE WHEN NOW() < q.open_at  THEN 'upcoming'
                       WHEN NOW() > q.close_at THEN 'closed'
                       ELSE 'open' END AS status
             FROM quizzes q
             JOIN courses c ON c.id = q.course_id
            WHERE c.lecturer_id = $1
            ORDER BY q.open_at DESC"#,
    )
    .bind(lecturer_id)
    .fetch_all(db.get_ref())
    .await
    .unwrap_or_default();

    let open_count = quizzes.iter().filter(|q| q.status == "open").count();
    let upcoming_count = quizzes.iter().filter(|q| q.status == "upcoming").count();
    let closed_count = quizzes.iter().filter(|q| q.status == "closed").count();

    let mut ctx = Context::new();
    base_ctx(&mut ctx, &user.display_name);
    ctx.insert("quizzes", &quizzes);
    ctx.insert("open_count", &open_count);
    ctx.insert("upcoming_count", &upcoming_count);
    ctx.insert("closed_count", &closed_count);
    render(&tmpl, "lecturer/quiz_manage.html", &ctx)
}

// GET /lecturer/quizzes/manage/new
pub async fn new_form(
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let lecturer_id = match lecturer_id_for_user(db.get_ref(), user.id).await {
        Ok(Some(id)) => id,
        Ok(None) => return HttpResponse::Forbidden().body("No lecturer record for this user."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };
    let courses = lecturer_courses(db.get_ref(), lecturer_id).await.unwrap_or_default();

    let mut ctx = Context::new();
    base_ctx(&mut ctx, &user.display_name);
    ctx.insert("mode", "new");
    ctx.insert("form_action", "/lecturer/quizzes/manage");
    ctx.insert("courses", &courses);
    render(&tmpl, "lecturer/quiz_form.html", &ctx)
}

// GET /lecturer/quizzes/manage/{id}/edit
pub async fn edit_form(
    path: web::Path<i32>,
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let quiz_id = path.into_inner();
    let lecturer_id = match lecturer_id_for_user(db.get_ref(), user.id).await {
        Ok(Some(id)) => id,
        Ok(None) => return HttpResponse::Forbidden().body("No lecturer record for this user."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };
    match quiz_owned(db.get_ref(), quiz_id, lecturer_id).await {
        Ok(true) => {}
        Ok(false) => return HttpResponse::Forbidden().body("You do not own this quiz."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    }

    #[derive(FromRow)]
    struct Head {
        course_id: i32,
        title: String,
        description: Option<String>,
        open_at: String,
        close_at: String,
        serve_count: Option<i32>,
        attempts_allowed: i32,
    }
    let head = match sqlx::query_as::<_, Head>(
        r#"SELECT course_id, title, description, serve_count, attempts_allowed,
                  to_char(open_at,  'YYYY-MM-DD"T"HH24:MI') AS open_at,
                  to_char(close_at, 'YYYY-MM-DD"T"HH24:MI') AS close_at
             FROM quizzes WHERE id = $1"#,
    )
    .bind(quiz_id)
    .fetch_one(db.get_ref())
    .await
    {
        Ok(h) => h,
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };

    #[derive(FromRow)]
    struct QRow {
        id: i32,
        question_text: String,
        question_type: String,
        marks: i32,
        difficulty: i16,
        topic: Option<String>,
    }
    let qrows = sqlx::query_as::<_, QRow>(
        "SELECT id, question_text, question_type, marks, difficulty, topic FROM quiz_questions WHERE quiz_id = $1 ORDER BY id",
    )
    .bind(quiz_id)
    .fetch_all(db.get_ref())
    .await
    .unwrap_or_default();

    let mut questions = Vec::with_capacity(qrows.len());
    for r in qrows {
        #[derive(FromRow)]
        struct ORow {
            option_text: String,
            is_correct: bool,
        }
        let opts = sqlx::query_as::<_, ORow>(
            "SELECT option_text, is_correct FROM quiz_options WHERE question_id = $1 ORDER BY id",
        )
        .bind(r.id)
        .fetch_all(db.get_ref())
        .await
        .unwrap_or_default();

        questions.push(EditQuestion {
            question_text: r.question_text,
            question_type: r.question_type,
            marks: r.marks,
            difficulty: r.difficulty,
            topic: r.topic.unwrap_or_default(),
            options: opts
                .into_iter()
                .map(|o| EditOption { option_text: o.option_text, is_correct: o.is_correct })
                .collect(),
        });
    }

    let edit = EditQuiz {
        id: quiz_id,
        course_id: head.course_id,
        title: head.title,
        description: head.description.unwrap_or_default(),
        open_at: head.open_at,
        close_at: head.close_at,
        serve_count: head.serve_count,
        attempts_allowed: head.attempts_allowed,
        questions,
    };
    let courses = lecturer_courses(db.get_ref(), lecturer_id).await.unwrap_or_default();

    let mut ctx = Context::new();
    base_ctx(&mut ctx, &user.display_name);
    ctx.insert("mode", "edit");
    ctx.insert("form_action", &format!("/lecturer/quizzes/manage/{quiz_id}"));
    ctx.insert("courses", &courses);
    ctx.insert("quiz", &edit); // template serialises with json_encode()
    render(&tmpl, "lecturer/quiz_form.html", &ctx)
}

// POST /lecturer/quizzes/manage (create)
pub async fn create(
    db: web::Data<PgPool>,
    session: Session,
    payload: web::Json<QuizInput>,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let lecturer_id = match lecturer_id_for_user(db.get_ref(), user.id).await {
        Ok(Some(id)) => id,
        Ok(None) => return HttpResponse::Forbidden().body("No lecturer record for this user."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };
    match course_owned(db.get_ref(), payload.course_id, lecturer_id).await {
        Ok(true) => {}
        Ok(false) => return json_err("You do not own the selected course."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    }
    let total = match validate(&payload) {
        Ok(t) => t,
        Err(msg) => return json_err(&msg),
    };
    match insert_quiz(db.get_ref(), user.id, total, &payload).await {
        Ok(_id) => HttpResponse::Ok().json(ApiResult {
            ok: true,
            message: "Quiz created.".into(),
            redirect: Some("/lecturer/quizzes/manage".into()),
        }),
        Err(e) => HttpResponse::InternalServerError().body(format!("Save failed: {e}")),
    }
}

// POST /lecturer/quizzes/manage/{id} (update)
pub async fn update(
    path: web::Path<i32>,
    db: web::Data<PgPool>,
    session: Session,
    payload: web::Json<QuizInput>,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let quiz_id = path.into_inner();
    let lecturer_id = match lecturer_id_for_user(db.get_ref(), user.id).await {
        Ok(Some(id)) => id,
        Ok(None) => return HttpResponse::Forbidden().body("No lecturer record for this user."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };
    match quiz_owned(db.get_ref(), quiz_id, lecturer_id).await {
        Ok(true) => {}
        Ok(false) => return json_err("You do not own this quiz."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    }
    match course_owned(db.get_ref(), payload.course_id, lecturer_id).await {
        Ok(true) => {}
        Ok(false) => return json_err("You do not own the selected course."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    }
    let total = match validate(&payload) {
        Ok(t) => t,
        Err(msg) => return json_err(&msg),
    };
    match update_quiz(db.get_ref(), quiz_id, total, &payload).await {
        Ok(()) => HttpResponse::Ok().json(ApiResult {
            ok: true,
            message: "Quiz updated.".into(),
            redirect: Some("/lecturer/quizzes/manage".into()),
        }),
        Err(e) => HttpResponse::InternalServerError().body(format!("Save failed: {e}")),
    }
}

// POST /lecturer/quizzes/manage/{id}/delete
pub async fn delete(
    path: web::Path<i32>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let quiz_id = path.into_inner();
    let lecturer_id = match lecturer_id_for_user(db.get_ref(), user.id).await {
        Ok(Some(id)) => id,
        Ok(None) => return HttpResponse::Forbidden().body("No lecturer record for this user."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };
    match quiz_owned(db.get_ref(), quiz_id, lecturer_id).await {
        Ok(true) => {}
        Ok(false) => return HttpResponse::Forbidden().body("You do not own this quiz."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    }
    if let Err(e) = sqlx::query("DELETE FROM quizzes WHERE id = $1")
        .bind(quiz_id)
        .execute(db.get_ref())
        .await
    {
        return HttpResponse::InternalServerError().body(format!("Delete failed: {e}"));
    }
    HttpResponse::SeeOther()
        .append_header(("Location", "/lecturer/quizzes/manage"))
        .finish()
}

// GET /lecturer/quizzes/manage/{id}/results
pub async fn results(
    path: web::Path<i32>,
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
) -> impl Responder {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(u) => u,
        Err(resp) => return resp,
    };
    let quiz_id = path.into_inner();
    let lecturer_id = match lecturer_id_for_user(db.get_ref(), user.id).await {
        Ok(Some(id)) => id,
        Ok(None) => return HttpResponse::Forbidden().body("No lecturer record for this user."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };
    match quiz_owned(db.get_ref(), quiz_id, lecturer_id).await {
        Ok(true) => {}
        Ok(false) => return HttpResponse::Forbidden().body("You do not own this quiz."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    }

    #[derive(FromRow)]
    struct Meta {
        title: String,
        course_code: String,
        total_marks: i32,
    }
    let meta = match sqlx::query_as::<_, Meta>(
        "SELECT q.title, c.course_code, q.total_marks FROM quizzes q JOIN courses c ON c.id = q.course_id WHERE q.id = $1",
    )
    .bind(quiz_id)
    .fetch_one(db.get_ref())
    .await
    {
        Ok(m) => m,
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };

    let attempts = sqlx::query_as::<_, AttemptRow>(
        r#"SELECT u.display_name AS student_name,
                  u.email        AS student_email,
                  to_char(qa.submitted_at, 'DD Mon YYYY, HH24:MI') AS submitted_at,
                  qa.score::float4 AS score,
                  CASE WHEN q.total_marks > 0 THEN (qa.score / q.total_marks * 100)::float4 ELSE 0 END AS percentage
             FROM quiz_attempts qa
             JOIN students s ON s.id = qa.student_id
             JOIN users    u ON u.id = s.user_id
             JOIN quizzes  q ON q.id = qa.quiz_id
            WHERE qa.quiz_id = $1 AND qa.submitted_at IS NOT NULL
            ORDER BY qa.submitted_at DESC"#,
    )
    .bind(quiz_id)
    .fetch_all(db.get_ref())
    .await
    .unwrap_or_default();

    let graded: Vec<&AttemptRow> = attempts.iter().filter(|a| a.score.is_some()).collect();
    let avg_pct: f32 = if graded.is_empty() {
        0.0
    } else {
        graded.iter().filter_map(|a| a.percentage).sum::<f32>() / graded.len() as f32
    };

    let mut ctx = Context::new();
    base_ctx(&mut ctx, &user.display_name);
    ctx.insert("quiz_title", &meta.title);
    ctx.insert("course_code", &meta.course_code);
    ctx.insert("total_marks", &meta.total_marks);
    ctx.insert("attempts", &attempts);
    ctx.insert("attempt_total", &(attempts.len() as i64));
    ctx.insert("avg_pct", &(avg_pct.round() as i32));
    render(&tmpl, "lecturer/quiz_results.html", &ctx)
}

// Register Routes. Call `.configure(lecturer_quiz::config)' in main.rs
pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/lecturer/quizzes/manage", web::get().to(manage_list))
        .route("/lecturer/quizzes/manage", web::post().to(create))
        .route("/lecturer/quizzes/manage/new", web::get().to(new_form))
        .route("/lecturer/quizzes/manage/{id}/edit", web::get().to(edit_form))
        .route("/lecturer/quizzes/manage/{id}/results", web::get().to(results))
        .route("/lecturer/quizzes/manage/{id}/delete", web::post().to(delete))       
        .route("/lecturer/quizzes/manage/{id}", web::post().to(update));
}

#[cfg(test)]
mod tests {
    use super::*;
    fn opt(t: &str, c: bool) -> OptionInput { OptionInput { option_text: t.into(), is_correct: c } }
    fn q() -> QuestionInput {
        QuestionInput { question_text: "2+2?".into(), question_type: "multiple_choice".into(), marks: 5,
            difficulty: 1, topic: None,
            options: vec![opt("4", true), opt("5", false)] }
    }
    fn base() -> QuizInput {
        QuizInput { course_id: 1, title: "Q1".into(), description: None,
            open_at: "2026-06-20T10:00".into(), close_at: "2026-06-21T10:00".into(),
            serve_count: None, attempts_allowed: 1, questions: vec![q()] }
    }
    #[test] fn totals() { let mut x = base(); x.questions.push(q()); assert_eq!(validate(&x).unwrap(), 10); }
    #[test] fn empty_title() { let mut x = base(); x.title = " ".into(); assert!(validate(&x).is_err()); }
    #[test] fn no_q() { let mut x = base(); x.questions.clear(); assert!(validate(&x).is_err()); }
    #[test] fn correct_count() {
        let mut x = base();
        x.questions[0].options = vec![opt("a", true), opt("b", true)];
        assert!(validate(&x).is_err());
    }
}