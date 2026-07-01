use actix_session::Session;
use actix_web::{HttpResponse, Responder, web};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use std::collections::BTreeMap;
use tera::{Context, Tera};

use crate::auth::UserRole;
use crate::quiz_engine;

const EWMA_ALPHA: f32 = 0.3; // learning rate, matches the graded engine

// ── DB helpers ──────────────────────────────────────────────────────────────

async fn student_id_for_user(db: &PgPool, user_id: i32) -> Result<Option<i32>, sqlx::Error> {
    sqlx::query_scalar::<_, i32>("SELECT id FROM students WHERE user_id = $1 LIMIT 1")
        .bind(user_id)
        .fetch_optional(db)
        .await
}

// True if the quiz exists, IS a practice quiz, and the student is enrolled.
async fn practice_access(db: &PgPool, quiz_id: i32, student_id: i32) -> Result<bool, sqlx::Error> {
    let found: Option<i32> = sqlx::query_scalar(
        r#"SELECT q.id
             FROM quizzes q
             JOIN enrollments e ON e.course_id = q.course_id
            WHERE q.id = $1 AND e.student_id = $2 AND q.is_practice = TRUE"#,
    )
    .bind(quiz_id)
    .bind(student_id)
    .fetch_optional(db)
    .await?;
    Ok(found.is_some())
}

async fn course_id_for_quiz(db: &PgPool, quiz_id: i32) -> Result<Option<i32>, sqlx::Error> {
    sqlx::query_scalar::<_, i32>("SELECT course_id FROM quizzes WHERE id = $1")
        .bind(quiz_id)
        .fetch_optional(db)
        .await
}

// Average practice proficiency across this student's course topics; defaults to 0.5 with no history.
async fn avg_practice_proficiency(db: &PgPool, student_id: i32, course_id: i32) -> f32 {
    sqlx::query_scalar::<_, Option<f32>>(
        r#"SELECT AVG(proficiency)::float4
             FROM student_practice_proficiency
            WHERE student_id = $1 AND course_id = $2"#,
    )
    .bind(student_id)
    .bind(course_id)
    .fetch_one(db)
    .await
    .ok()
    .flatten()
    .unwrap_or(0.5)
}

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

#[derive(FromRow)]
struct QuizMeta {
    title: String,
    course_code: String,
    course_name: String,
}

async fn quiz_meta(db: &PgPool, quiz_id: i32) -> Result<Option<QuizMeta>, sqlx::Error> {
    sqlx::query_as::<_, QuizMeta>(
        r#"SELECT q.title, c.course_code, c.course_name
             FROM quizzes q JOIN courses c ON c.id = q.course_id WHERE q.id = $1"#,
    )
    .bind(quiz_id)
    .fetch_optional(db)
    .await
}

fn level_for(p: f32) -> &'static str {
    if p >= 0.75 {
        "Advanced"
    } else if p >= 0.45 {
        "Intermediate"
    } else {
        "Beginner"
    }
}

// Create a practice attempt: pick a difficulty-targeted subset from the bank, freeze it, return attempt id.
async fn create_practice_attempt(
    db: &PgPool,
    quiz_id: i32,
    student_id: i32,
    course_id: i32,
) -> Result<Option<i32>, sqlx::Error> {
    let pool = quiz_engine::load_questions(db, quiz_id).await?;
    if pool.is_empty() {
        return Ok(None);
    }

    let serve_cfg: Option<i32> =
        sqlx::query_scalar::<_, Option<i32>>("SELECT serve_count FROM quizzes WHERE id = $1")
            .bind(quiz_id)
            .fetch_one(db)
            .await?;
    let serve = serve_cfg.map(|n| n.max(1) as usize).unwrap_or(pool.len());

    // Step 1 + 2: proficiency -> target difficulty -> bank subset.
    let proficiency = avg_practice_proficiency(db, student_id, course_id).await;
    let target = quiz_engine::select_next_difficulty(proficiency, 0);

    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
        ^ (((student_id as u64) << 32) | quiz_id as u64);

    let ids = quiz_engine::select_practice_subset_ids(&pool, serve, target, seed);

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

fn render(tmpl: &Tera, name: &str, ctx: &Context) -> HttpResponse {
    match tmpl.render(name, ctx) {
        Ok(html) => HttpResponse::Ok().content_type("text/html").body(html),
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
    }
}

// ── GET /student/practice
#[derive(Serialize)]
struct PracticeCard {
    id: i32,
    title: String,
    course_code: String,
    course_name: String,
    bank_size: i64,
    serve_count: i32,
    attempts_used: i64,
    last_score: Option<i32>,
    last_total: Option<i32>,
    proficiency_pct: i32,
    level: String,
}

pub async fn practice_list(
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
            crate::insert_student_base(&mut ctx, &user.display_name, "");
            ctx.insert("active_page", "practice");
            ctx.insert("quizzes", &Vec::<PracticeCard>::new());
            return render(&tmpl, "student/quiz_practice.html", &ctx);
        }
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };

    #[derive(FromRow)]
    struct Row {
        id: i32,
        course_id: i32,
        title: String,
        course_code: String,
        course_name: String,
        bank_size: i64,
        serve_count: Option<i32>,
        attempts_used: i64,
        last_score: Option<f32>,
        last_total: Option<i32>,
    }
    let rows = sqlx::query_as::<_, Row>(
        r#"SELECT q.id, q.course_id, q.title, c.course_code, c.course_name,
                  (SELECT COUNT(*) FROM quiz_questions qq WHERE qq.quiz_id = q.id) AS bank_size,
                  q.serve_count,
                  (SELECT COUNT(*) FROM quiz_attempts a
                     WHERE a.quiz_id = q.id AND a.student_id = $1 AND a.submitted_at IS NOT NULL) AS attempts_used,
                  (SELECT a.score::float4 FROM quiz_attempts a
                     WHERE a.quiz_id = q.id AND a.student_id = $1 AND a.submitted_at IS NOT NULL
                     ORDER BY a.submitted_at DESC LIMIT 1) AS last_score,
                  (SELECT a.total_marks FROM quiz_attempts a
                     WHERE a.quiz_id = q.id AND a.student_id = $1 AND a.submitted_at IS NOT NULL
                     ORDER BY a.submitted_at DESC LIMIT 1) AS last_total
             FROM quizzes q
             JOIN courses c ON c.id = q.course_id
             JOIN enrollments e ON e.course_id = c.id
            WHERE e.student_id = $1 AND q.is_practice = TRUE
            ORDER BY c.course_code, q.title"#,
    )
    .bind(student_id)
    .fetch_all(db.get_ref())
    .await
    .unwrap_or_default();

    let mut cards = Vec::with_capacity(rows.len());
    for r in &rows {
        let prof = avg_practice_proficiency(db.get_ref(), student_id, r.course_id).await;
        cards.push(PracticeCard {
            id: r.id,
            title: r.title.clone(),
            course_code: r.course_code.clone(),
            course_name: r.course_name.clone(),
            bank_size: r.bank_size,
            serve_count: r.serve_count.unwrap_or(r.bank_size as i32),
            attempts_used: r.attempts_used,
            last_score: r.last_score.map(|s| s.round() as i32),
            last_total: r.last_total,
            proficiency_pct: (prof * 100.0).round() as i32,
            level: level_for(prof).into(),
        });
    }

    crate::insert_student_base(&mut ctx, &user.display_name, &student_id.to_string());
    ctx.insert("active_page", "practice");
    ctx.insert("quizzes", &cards);
    render(&tmpl, "student/quiz_practice.html", &ctx)
}

// ── GET /student/practice/{quiz_id}/take
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
    difficulty: i16,
    options: Vec<TakeOption>,
}

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
    match practice_access(db.get_ref(), quiz_id, student_id).await {
        Ok(true) => {}
        Ok(false) => return HttpResponse::NotFound().body("Practice quiz not available."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    }
    let course_id = match course_id_for_quiz(db.get_ref(), quiz_id).await {
        Ok(Some(id)) => id,
        Ok(None) => return HttpResponse::NotFound().body("Quiz not found."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };

    // Current proficiency BEFORE this attempt (shown to the student).
    let proficiency = avg_practice_proficiency(db.get_ref(), student_id, course_id).await;
    let target_difficulty = quiz_engine::select_next_difficulty(proficiency, 0);

    // Resume an in-progress attempt, or start a fresh adaptive one.
    let attempt_id = match in_progress_attempt(db.get_ref(), quiz_id, student_id).await {
        Ok(Some(id)) => id,
        Ok(None) => match create_practice_attempt(db.get_ref(), quiz_id, student_id, course_id)
            .await
        {
            Ok(Some(id)) => id,
            Ok(None) => {
                return HttpResponse::BadRequest().body("This practice quiz has no questions yet.");
            }
            Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
        },
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };

    let meta = match quiz_meta(db.get_ref(), quiz_id).await {
        Ok(Some(m)) => m,
        Ok(None) => return HttpResponse::NotFound().body("Quiz not found."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };

    let served = match quiz_engine::load_served_questions(db.get_ref(), attempt_id).await {
        Ok(q) => q,
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };
    if served.is_empty() {
        return HttpResponse::BadRequest().body("This practice quiz has no questions yet.");
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
            difficulty: q.difficulty,
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

    let mut ctx = Context::new();
    crate::insert_student_base(&mut ctx, &user.display_name, &student_id.to_string());
    ctx.insert("active_page", "practice");
    ctx.insert("quiz_id", &quiz_id);
    ctx.insert("quiz_title", &meta.title);
    ctx.insert("course_code", &meta.course_code);
    ctx.insert("course_name", &meta.course_name);
    ctx.insert("total_marks", &served_total);
    ctx.insert("questions", &questions);
    ctx.insert("proficiency_pct", &((proficiency * 100.0).round() as i32));
    ctx.insert("proficiency_level", level_for(proficiency));
    ctx.insert("target_difficulty", &target_difficulty);
    ctx.insert("submit_url", &format!("/student/practice/{quiz_id}/submit"));
    render(&tmpl, "student/quiz_practice_take.html", &ctx)
}

// ── POST /student/practice/{quiz_id}/submit
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

fn submit_err(msg: &str) -> HttpResponse {
    HttpResponse::BadRequest().json(SubmitResult {
        ok: false,
        message: msg.into(),
        redirect: None,
    })
}

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
    match practice_access(db.get_ref(), quiz_id, student_id).await {
        Ok(true) => {}
        Ok(false) => return HttpResponse::NotFound().body("Practice quiz not available."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    }
    let course_id = match course_id_for_quiz(db.get_ref(), quiz_id).await {
        Ok(Some(id)) => id,
        Ok(None) => return HttpResponse::NotFound().body("Quiz not found."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };

    let attempt_id = match in_progress_attempt(db.get_ref(), quiz_id, student_id).await {
        Ok(Some(id)) => id,
        Ok(None) => return submit_err("No active attempt - start the practice quiz first."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };

    let questions = match quiz_engine::load_served_questions(db.get_ref(), attempt_id).await {
        Ok(q) if !q.is_empty() => q,
        Ok(_) => return submit_err("This attempt has no questions."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };

    let submission = quiz_engine::AttemptSubmission {
        answers: payload
            .answers
            .iter()
            .map(|a| quiz_engine::AnswerSubmission {
                question_id: a.question_id,
                selected_option_id: a.selected_option_id,
            })
            .collect(),
    };
    let result = quiz_engine::grade_attempt(&questions, &submission);

    // Persist attempt + answers + practice proficiency (single transaction).
    if let Err(e) = persist_practice_attempt(
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

    HttpResponse::Ok().json(SubmitResult {
        ok: true,
        message: "Submitted.".into(),
        redirect: Some(format!("/student/practice/{quiz_id}/result")),
    })
}

// Finalize a practice attempt, update per-topic proficiency, and record before/after snapshots for the result page.
async fn persist_practice_attempt(
    db: &PgPool,
    attempt_id: i32,
    student_id: i32,
    course_id: i32,
    submission: &quiz_engine::AttemptSubmission,
    result: &quiz_engine::AttemptResult,
) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;

    sqlx::query(
        r#"UPDATE quiz_attempts
              SET submitted_at = NOW(),
                  score        = CAST($2 AS NUMERIC),
                  total_marks  = $3
            WHERE id = $1"#,
    )
    .bind(attempt_id)
    .bind(result.score)
    .bind(result.total_marks)
    .execute(&mut *tx)
    .await?;

    // topic -> (before, running, answered_in_this_attempt)
    let mut state: BTreeMap<String, (f32, f32, i32)> = BTreeMap::new();

    for g in &result.graded {
        let sub = submission
            .answers
            .iter()
            .find(|a| a.question_id == g.question_id);
        sqlx::query(
            r#"INSERT INTO quiz_answers
                   (attempt_id, question_id, selected_option_id, is_correct)
               VALUES ($1, $2, $3, $4)"#,
        )
        .bind(attempt_id)
        .bind(g.question_id)
        .bind(sub.and_then(|s| s.selected_option_id))
        .bind(g.is_correct)
        .execute(&mut *tx)
        .await?;

        if let Some(topic) = &g.topic {
            if !state.contains_key(topic) {
                let current: Option<f32> = sqlx::query_scalar(
                    r#"SELECT proficiency::float4 FROM student_practice_proficiency
                        WHERE student_id = $1 AND course_id = $2 AND topic = $3"#,
                )
                .bind(student_id)
                .bind(course_id)
                .bind(topic)
                .fetch_optional(&mut *tx)
                .await?;
                let c = current.unwrap_or(0.5);
                state.insert(topic.clone(), (c, c, 0));
            }
            let entry = state.get_mut(topic).unwrap();
            entry.1 = quiz_engine::update_proficiency(entry.1, g.is_correct, EWMA_ALPHA);
            entry.2 += 1;
        }
    }

    // Upsert the new proficiency and write the before/after snapshot per topic.
    for (topic, (before, after, answered)) in &state {
        sqlx::query(
            r#"INSERT INTO student_practice_proficiency
                   (student_id, course_id, topic, proficiency, answered_count, updated_at)
               VALUES ($1, $2, $3, CAST($4 AS NUMERIC), $5, NOW())
               ON CONFLICT (student_id, course_id, topic) DO UPDATE
                 SET proficiency    = EXCLUDED.proficiency,
                     answered_count = student_practice_proficiency.answered_count + EXCLUDED.answered_count,
                     updated_at     = NOW()"#,
        )
        .bind(student_id)
        .bind(course_id)
        .bind(topic)
        .bind(*after as f64)
        .bind(*answered)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"INSERT INTO practice_attempt_proficiency (attempt_id, topic, prof_before, prof_after)
               VALUES ($1, $2, CAST($3 AS NUMERIC), CAST($4 AS NUMERIC))
               ON CONFLICT (attempt_id, topic) DO UPDATE
                 SET prof_before = EXCLUDED.prof_before,
                     prof_after  = EXCLUDED.prof_after"#,
        )
        .bind(attempt_id)
        .bind(topic)
        .bind(*before as f64)
        .bind(*after as f64)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

// ── GET /student/practice/{quiz_id}/result
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
    match practice_access(db.get_ref(), quiz_id, student_id).await {
        Ok(true) => {}
        Ok(false) => return HttpResponse::NotFound().body("Practice quiz not available."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    }
    let meta = match quiz_meta(db.get_ref(), quiz_id).await {
        Ok(Some(m)) => m,
        Ok(None) => return HttpResponse::NotFound().body("Quiz not found."),
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };
    let course_id = course_id_for_quiz(db.get_ref(), quiz_id)
        .await
        .ok()
        .flatten()
        .unwrap_or(0);

    #[derive(FromRow)]
    struct AttemptRow {
        id: i32,
        score: Option<f32>,
        total_marks: Option<i32>,
        submitted_at: Option<String>,
    }
    let attempt = match sqlx::query_as::<_, AttemptRow>(
        r#"SELECT id, score::float4 AS score, total_marks,
                  to_char(submitted_at, 'DD Mon YYYY, HH24:MI') AS submitted_at
             FROM quiz_attempts
            WHERE quiz_id = $1 AND student_id = $2 AND submitted_at IS NOT NULL
            ORDER BY submitted_at DESC LIMIT 1"#,
    )
    .bind(quiz_id)
    .bind(student_id)
    .fetch_optional(db.get_ref())
    .await
    {
        Ok(Some(a)) => a,
        Ok(None) => {
            return HttpResponse::SeeOther()
                .insert_header(("Location", format!("/student/practice/{quiz_id}/take")))
                .finish();
        }
        Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
    };

    // Per-question breakdown.
    #[derive(FromRow)]
    struct BreakRow {
        question_text: String,
        marks: i32,
        is_correct: bool,
        your_answer: Option<String>,
        correct_answer: Option<String>,
    }
    let rows = sqlx::query_as::<_, BreakRow>(
        r#"SELECT qq.question_text, qq.marks, ans.is_correct,
                  chosen.option_text AS your_answer,
                  correct.option_text AS correct_answer
             FROM quiz_attempt_questions aq
             JOIN quiz_questions qq ON qq.id = aq.question_id
             LEFT JOIN quiz_answers ans ON ans.question_id = qq.id AND ans.attempt_id = $1
             LEFT JOIN quiz_options chosen ON chosen.id = ans.selected_option_id
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

    let total = attempt.total_marks.unwrap_or(0);
    let score = attempt.score.unwrap_or(0.0).round() as i32;
    let percentage = if total > 0 {
        (score as f32 / total as f32 * 100.0).round() as i32
    } else {
        0
    };

    // Before/after proficiency move for THIS attempt (Step 3).
    #[derive(FromRow)]
    struct MoveRow {
        topic: String,
        prof_before: f32,
        prof_after: f32,
    }
    let move_rows = sqlx::query_as::<_, MoveRow>(
        r#"SELECT topic, prof_before::float4 AS prof_before, prof_after::float4 AS prof_after
             FROM practice_attempt_proficiency
            WHERE attempt_id = $1
            ORDER BY topic"#,
    )
    .bind(attempt.id)
    .fetch_all(db.get_ref())
    .await
    .unwrap_or_default();

    #[derive(Serialize)]
    struct ProfMove {
        topic: String,
        before_pct: i32,
        after_pct: i32,
        delta_pct: i32,
        improved: bool,
        level: String,
    }
    let moves: Vec<ProfMove> = move_rows
        .iter()
        .map(|m| {
            let before_pct = (m.prof_before * 100.0).round() as i32;
            let after_pct = (m.prof_after * 100.0).round() as i32;
            ProfMove {
                topic: m.topic.clone(),
                before_pct,
                after_pct,
                delta_pct: after_pct - before_pct,
                improved: after_pct >= before_pct,
                level: level_for(m.prof_after).into(),
            }
        })
        .collect();

    let overall_after = avg_practice_proficiency(db.get_ref(), student_id, course_id).await;

    let mut ctx = Context::new();
    crate::insert_student_base(&mut ctx, &user.display_name, &student_id.to_string());
    ctx.insert("active_page", "practice");
    ctx.insert("quiz_id", &quiz_id);
    ctx.insert("quiz_title", &meta.title);
    ctx.insert("course_code", &meta.course_code);
    ctx.insert("score", &score);
    ctx.insert("total_marks", &total);
    ctx.insert("percentage", &percentage);
    ctx.insert("submitted_at", &attempt.submitted_at.unwrap_or_default());
    ctx.insert("questions", &questions);
    ctx.insert("moves", &moves);
    ctx.insert("overall_pct", &((overall_after * 100.0).round() as i32));
    ctx.insert("overall_level", level_for(overall_after));
    render(&tmpl, "student/quiz_practice_result.html", &ctx)
}

// Register practice routes. Call `.configure(student_practice::config)` in main.rs.
pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/student/practice", web::get().to(practice_list))
        .route("/student/practice/{quiz_id}/take", web::get().to(take))
        .route("/student/practice/{quiz_id}/submit", web::post().to(submit))
        .route("/student/practice/{quiz_id}/result", web::get().to(result));
}
