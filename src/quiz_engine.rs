use actix_session::Session;
use actix_web::{web, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};

use crate::auth::UserRole;

// One quiz question bundled with its answer options.
#[derive(Debug, Clone, Serialize)]
pub struct EngineQuestion {
	pub id: i32,
	pub prompt: String,
	pub question_type: String, // 'multiple_choice' | 'true_false'
	pub marks: i32,
	pub difficulty: i16,       // 1..=5
	pub topic: Option<String>,
	pub options: Vec<EngineOption>,
}

// One answer choice (from `quiz_options`). Holds `is_correct` for server-side
// grading, but that flag is hidden when the option is sent to the student.
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct EngineOption {
	pub id: i32,
	pub option_text: String,
	#[serde(skip_serializing)]
	pub is_correct: bool,
}

// One answer submitted by the student for a single (option-based) question.
#[derive(Debug, Clone, Deserialize)]
pub struct AnswerSubmission {
	pub question_id: i32,
	pub selected_option_id: Option<i32>,
}

// Full attempt submission payload (POST body).
#[derive(Debug, Clone, Deserialize)]
pub struct AttemptSubmission {
	pub answers: Vec<AnswerSubmission>,
}

// Result of grading one question.
#[derive(Debug, Clone, Serialize)]
pub struct GradedAnswer {
	pub question_id: i32,
	pub is_correct: bool,
	pub marks_awarded: i32,
	pub marks_possible: i32,
	pub topic: Option<String>,
}

// Result of grading a whole attempt.
#[derive(Debug, Clone, Serialize)]
pub struct AttemptResult {
	pub score: i32,
	pub total_marks: i32,
	pub percentage: f32,
	pub graded: Vec<GradedAnswer>,
}

// A recommended piece of course material for a weak topic.
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct MaterialRecommendation {
	pub material_id: i32,
	pub title: String,
	pub material_type: String,
	pub file_path: String,
	pub topic: Option<String>,
}


// Pure engine logic 

// Grade a single answer against its question's options.
// Quizzes are option-based only (multiple_choice / true_false).
pub fn grade_answer(question: &EngineQuestion, submission: &AnswerSubmission) -> GradedAnswer {
	let is_correct = match question.question_type.as_str() {
		"multiple_choice" | "true_false" => submission
			.selected_option_id
			.map(|chosen| {
				question
					.options
					.iter()
					.any(|o| o.id == chosen && o.is_correct)
			})
			.unwrap_or(false),
		_ => false, // any non-option-based type scores 0
	};

	GradedAnswer {
		question_id: question.id,
		is_correct,
		marks_awarded: if is_correct { question.marks } else { 0 },
		marks_possible: question.marks,
		topic: question.topic.clone(),
	}
}

// Grade an entire attempt. Questions without a matching submission are scored 0.
pub fn grade_attempt(
	questions: &[EngineQuestion],
	submission: &AttemptSubmission,
) -> AttemptResult {
	let mut graded = Vec::with_capacity(questions.len());
	let mut score = 0;
	let mut total = 0;

	for q in questions {
		total += q.marks;
		let blank = AnswerSubmission {
			question_id: q.id,
			selected_option_id: None,
		};
		let ans = submission
			.answers
			.iter()
			.find(|a| a.question_id == q.id)
			.unwrap_or(&blank);
		let g = grade_answer(q, ans);
		score += g.marks_awarded;
		graded.push(g);
	}

	let percentage = if total > 0 {
		(score as f32 / total as f32) * 100.0
	} else {
		0.0
	};

	AttemptResult {
		score,
		total_marks: total,
		percentage,
		graded,
	}
}

// Exponential moving average update of a topic proficiency in 0.0..=1.0.
// `alpha` is the learning rate (how much the newest answer moves the estimate).
pub fn update_proficiency(current: f32, was_correct: bool, alpha: f32) -> f32 {
	let target = if was_correct { 1.0 } else { 0.0 };
	let next = current + alpha * (target - current);
	next.clamp(0.0, 1.0)
}

// Map a proficiency estimate to the difficulty (1..=5) of the next question.
// Weak students get easier questions; strong students get harder ones. The
// `streak` (consecutive correct answers, negative for wrong) nudges it.
pub fn select_next_difficulty(proficiency: f32, streak: i32) -> i16 {
	// proficiency 0.0..1.0 -> base band 1..5
	let base = (proficiency * 4.0).round() as i32 + 1; // 1..=5
	let adjusted = base + streak.clamp(-2, 2);
	adjusted.clamp(1, 5) as i16
}

/// Pick the next question for an adaptive session: from the not-yet-answered
/// pool, choose the one whose difficulty is closest to the target. Returns a
/// reference index into `pool`, or None if the pool is empty.
pub fn pick_adaptive_index(pool: &[EngineQuestion], target_difficulty: i16) -> Option<usize> {
	pool.iter()
		.enumerate()
		.min_by_key(|(_, q)| (q.difficulty - target_difficulty).abs())
		.map(|(i, _)| i)
}


// DB access

// Resolve the students.id for the logged-in user, or None if not a student row.
async fn student_id_for_user(db: &PgPool, user_id: i32) -> Result<Option<i32>, sqlx::Error> {
	sqlx::query_scalar::<_, i32>("SELECT id FROM students WHERE user_id = $1 LIMIT 1")
		.bind(user_id)
		.fetch_optional(db)
		.await
}

// Load all questions (with options) for a quiz.
pub async fn load_questions(db: &PgPool, quiz_id: i32) -> Result<Vec<EngineQuestion>, sqlx::Error> {
	#[derive(FromRow)]
	struct QRow {
		id: i32,
		question_text: String,
		question_type: String,
		marks: i32,
		difficulty: i16,
		topic: Option<String>,
	}

	let rows = sqlx::query_as::<_, QRow>(
		r#"SELECT id, question_text, question_type, marks, difficulty, topic
		     FROM quiz_questions
		    WHERE quiz_id = $1
		    ORDER BY id"#,
	)
	.bind(quiz_id)
	.fetch_all(db)
	.await?;

	let mut questions = Vec::with_capacity(rows.len());
	for r in rows {
		let options = sqlx::query_as::<_, EngineOption>(
			r#"SELECT id, option_text, is_correct
			     FROM quiz_options
			    WHERE question_id = $1
			    ORDER BY id"#,
		)
		.bind(r.id)
		.fetch_all(db)
		.await?;

		questions.push(EngineQuestion {
			id: r.id,
			prompt: r.question_text,
			question_type: r.question_type,
			marks: r.marks,
			difficulty: r.difficulty,
			topic: r.topic,
			options,
		});
	}
	Ok(questions)
}

// Persist a graded attempt: create the attempt row, store each answer, set the
// final score, and update topic proficiencies. Returns the new attempt id.
pub async fn persist_attempt(
	db: &PgPool,
	quiz_id: i32,
	student_id: i32,
	course_id: i32,
	submission: &AttemptSubmission,
	result: &AttemptResult,
) -> Result<i32, sqlx::Error> {
	let mut tx = db.begin().await?;

	// score is an integer count of marks; CAST in SQL so we can bind an i32
	// rather than depend on rust_decimal for the NUMERIC column.
	let attempt_id: i32 = sqlx::query_scalar(
		r#"INSERT INTO quiz_attempts (quiz_id, student_id, submitted_at, score)
		   VALUES ($1, $2, NOW(), CAST($3 AS NUMERIC))
		   RETURNING id"#,
	)
	.bind(quiz_id)
	.bind(student_id)
	.bind(result.score)
	.fetch_one(&mut *tx)
	.await?;

	for g in &result.graded {
		let sub = submission.answers.iter().find(|a| a.question_id == g.question_id);
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

		// Update proficiency for the question's topic (EWMA upsert).
		if let Some(topic) = &g.topic {
			let current: Option<f32> = sqlx::query_scalar(
				r#"SELECT proficiency::float4 FROM student_topic_proficiency
				    WHERE student_id = $1 AND course_id = $2 AND topic = $3"#,
			)
			.bind(student_id)
			.bind(course_id)
			.bind(topic)
			.fetch_optional(&mut *tx)
			.await?;

			let next = update_proficiency(current.unwrap_or(0.5), g.is_correct, 0.3);
			sqlx::query(
				r#"INSERT INTO student_topic_proficiency
				       (student_id, course_id, topic, proficiency, answered_count, updated_at)
				   VALUES ($1, $2, $3, CAST($4 AS NUMERIC), 1, NOW())
				   ON CONFLICT (student_id, course_id, topic) DO UPDATE
				     SET proficiency    = EXCLUDED.proficiency,
				         answered_count = student_topic_proficiency.answered_count + 1,
				         updated_at     = NOW()"#,
			)
			.bind(student_id)
			.bind(course_id)
			.bind(topic)
			.bind(next as f64)
			.execute(&mut *tx)
			.await?;
		}
	}

	tx.commit().await?;
	Ok(attempt_id)
}

// Recommend course materials for the student's weakest topics in a course.
pub async fn recommend_materials(
	db: &PgPool,
	student_id: i32,
	course_id: i32,
	threshold: f32,
	limit: i64,
) -> Result<Vec<MaterialRecommendation>, sqlx::Error> {
	sqlx::query_as::<_, MaterialRecommendation>(
		r#"SELECT cm.id   AS material_id,
		          cm.title,
		          cm.material_type,
		          cm.file_path,
		          cm.topic
		     FROM student_topic_proficiency stp
		     JOIN course_materials cm
		       ON cm.course_id = stp.course_id
		      AND cm.topic     = stp.topic
		    WHERE stp.student_id = $1
		      AND stp.course_id  = $2
		      AND stp.proficiency < $3
		    ORDER BY stp.proficiency ASC
		    LIMIT $4"#,
	)
	.bind(student_id)
	.bind(course_id)
	.bind(threshold as f64)
	.bind(limit)
	.fetch_all(db)
	.await
}

// Look up the course a quiz belongs to.
async fn course_id_for_quiz(db: &PgPool, quiz_id: i32) -> Result<Option<i32>, sqlx::Error> {
	sqlx::query_scalar::<_, i32>("SELECT course_id FROM quizzes WHERE id = $1")
		.bind(quiz_id)
		.fetch_optional(db)
		.await
}

// Timing/limit info for one quiz. The open/close checks are computed in SQL
// (`NOW()`) so we don't need a Rust timestamp type to compare them.
#[derive(FromRow)]
struct QuizGate {
	is_before_open: bool,
	is_after_close: bool,
	attempts_allowed: i32,
}

// Fetch the attempt window + allowed-attempt count for a quiz.
async fn quiz_gate(db: &PgPool, quiz_id: i32) -> Result<Option<QuizGate>, sqlx::Error> {
	sqlx::query_as::<_, QuizGate>(
		r#"SELECT (NOW() < open_at) AS is_before_open,
		          (NOW() > close_at) AS is_after_close,
		          attempts_allowed
		     FROM quizzes
		    WHERE id = $1"#,
	)
	.bind(quiz_id)
	.fetch_optional(db)
	.await
}

// Count how many attempts this student has already made on this quiz.
async fn count_attempts(db: &PgPool, quiz_id: i32, student_id: i32) -> Result<i64, sqlx::Error> {
	sqlx::query_scalar::<_, i64>(
		"SELECT COUNT(*) FROM quiz_attempts WHERE quiz_id = $1 AND student_id = $2",
	)
	.bind(quiz_id)
	.bind(student_id)
	.fetch_one(db)
	.await
}

// HTTP handlers

#[derive(Serialize)]
struct SubmitResponse {
	attempt_id: i32,
	result: AttemptResult,
	recommendations: Vec<MaterialRecommendation>,
}

// POST /student/quizzes/{quiz_id}/submit
// Grades the submitted answers, persists the attempt, updates proficiency,
// and returns the score plus material recommendations for weak topics.
pub async fn submit_quiz_attempt(
	path: web::Path<i32>,
	db: web::Data<PgPool>,
	session: Session,
	payload: web::Json<AttemptSubmission>,
) -> impl Responder {
	let user = match crate::auth::require_role(&session, UserRole::Student) {
		Ok(user) => user,
		Err(response) => return response,
	};
	let quiz_id = path.into_inner();

	let student_id = match student_id_for_user(db.get_ref(), user.id).await {
		Ok(Some(id)) => id,
		Ok(None) => return HttpResponse::Forbidden().body("No student record for this user."),
		Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
	};

	let course_id = match course_id_for_quiz(db.get_ref(), quiz_id).await {
		Ok(Some(id)) => id,
		Ok(None) => return HttpResponse::NotFound().body("Quiz not found."),
		Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
	};

	// Enforce the attempt window and the attempt-count limit before grading.
	let gate = match quiz_gate(db.get_ref(), quiz_id).await {
		Ok(Some(g)) => g,
		Ok(None) => return HttpResponse::NotFound().body("Quiz not found."),
		Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
	};
	if gate.is_before_open {
		return HttpResponse::Forbidden().body("This quiz is not open yet.");
	}
	if gate.is_after_close {
		return HttpResponse::Forbidden().body("This quiz has closed.");
	}

	let attempts_used = match count_attempts(db.get_ref(), quiz_id, student_id).await {
		Ok(n) => n,
		Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
	};
	if attempts_used >= gate.attempts_allowed as i64 {
		return HttpResponse::Forbidden().body("No attempts remaining for this quiz.");
	}

	let questions = match load_questions(db.get_ref(), quiz_id).await {
		Ok(q) if !q.is_empty() => q,
		Ok(_) => return HttpResponse::BadRequest().body("Quiz has no questions."),
		Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
	};

	let result = grade_attempt(&questions, &payload);

	let attempt_id = match persist_attempt(
		db.get_ref(),
		quiz_id,
		student_id,
		course_id,
		&payload,
		&result,
	)
	.await
	{
		Ok(id) => id,
		Err(e) => return HttpResponse::InternalServerError().body(format!("Save failed: {e}")),
	};

	let recommendations = recommend_materials(db.get_ref(), student_id, course_id, 0.6, 5)
		.await
		.unwrap_or_default();

	HttpResponse::Ok().json(SubmitResponse {
		attempt_id,
		result,
		recommendations,
	})
}

#[derive(Deserialize)]
pub struct NextQuestionQuery {
	pub answered: Option<String>,
	pub streak: Option<i32>,
}

// GET /student/quizzes/{quiz_id}/next?answered=1,2&streak=1
// Returns the next adaptive question (answer key stripped) based on the
// student's proficiency in this course and their current streak.
pub async fn next_adaptive_question(
	path: web::Path<i32>,
	query: web::Query<NextQuestionQuery>,
	db: web::Data<PgPool>,
	session: Session,
) -> impl Responder {
	let user = match crate::auth::require_role(&session, UserRole::Student) {
		Ok(user) => user,
		Err(response) => return response,
	};
	let quiz_id = path.into_inner();

	let student_id = match student_id_for_user(db.get_ref(), user.id).await {
		Ok(Some(id)) => id,
		Ok(None) => return HttpResponse::Forbidden().body("No student record for this user."),
		Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
	};
	let course_id = match course_id_for_quiz(db.get_ref(), quiz_id).await {
		Ok(Some(id)) => id,
		Ok(None) => return HttpResponse::NotFound().body("Quiz not found."),
		Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
	};

	// Average proficiency across this student's topics in the course (default 0.5).
	let proficiency: f32 = sqlx::query_scalar::<_, Option<f32>>(
		r#"SELECT AVG(proficiency)::float4
		     FROM student_topic_proficiency
		    WHERE student_id = $1 AND course_id = $2"#,
	)
	.bind(student_id)
	.bind(course_id)
	.fetch_one(db.get_ref())
	.await
	.ok()
	.flatten()
	.unwrap_or(0.5);

	let target = select_next_difficulty(proficiency, query.streak.unwrap_or(0));

	let answered: Vec<i32> = query
		.answered
		.as_deref()
		.unwrap_or("")
		.split(',')
		.filter_map(|s| s.trim().parse::<i32>().ok())
		.collect();

	let all = match load_questions(db.get_ref(), quiz_id).await {
		Ok(q) => q,
		Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
	};
	let pool: Vec<EngineQuestion> = all
		.into_iter()
		.filter(|q| !answered.contains(&q.id))
		.collect();

	#[derive(Serialize)]
	struct Done {
		done: bool,
	}

	match pick_adaptive_index(&pool, target) {
		Some(i) => HttpResponse::Ok().json(&pool[i]), // is_correct is skip_serialized
		None => HttpResponse::Ok().json(Done { done: true }),
	}
}

// GET /student/quizzes/{quiz_id}/recommendations
pub async fn quiz_recommendations(
	path: web::Path<i32>,
	db: web::Data<PgPool>,
	session: Session,
) -> impl Responder {
	let user = match crate::auth::require_role(&session, UserRole::Student) {
		Ok(user) => user,
		Err(response) => return response,
	};
	let quiz_id = path.into_inner();

	let student_id = match student_id_for_user(db.get_ref(), user.id).await {
		Ok(Some(id)) => id,
		Ok(None) => return HttpResponse::Forbidden().body("No student record for this user."),
		Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
	};
	let course_id = match course_id_for_quiz(db.get_ref(), quiz_id).await {
		Ok(Some(id)) => id,
		Ok(None) => return HttpResponse::NotFound().body("Quiz not found."),
		Err(e) => return HttpResponse::InternalServerError().body(format!("DB error: {e}")),
	};

	match recommend_materials(db.get_ref(), student_id, course_id, 0.6, 10).await {
		Ok(recs) => HttpResponse::Ok().json(recs),
		Err(e) => HttpResponse::InternalServerError().body(format!("DB error: {e}")),
	}
}

// Register engine routes. Call `.configure(quiz_engine::config)` in main.rs.
pub fn config(cfg: &mut web::ServiceConfig) {
	cfg.route(
		"/student/quizzes/{quiz_id}/submit",
		web::post().to(submit_quiz_attempt),
	)
	.route(
		"/student/quizzes/{quiz_id}/next",
		web::get().to(next_adaptive_question),
	)
	.route(
		"/student/quizzes/{quiz_id}/recommendations",
		web::get().to(quiz_recommendations),
	);
}


// Unit tests for the pure logigitggggg
#[cfg(test)]
mod tests {
	use super::*;

	fn mcq(id: i32, marks: i32, difficulty: i16, correct_id: i32) -> EngineQuestion {
		EngineQuestion {
			id,
			prompt: "q".into(),
			question_type: "multiple_choice".into(),
			marks,
			difficulty,
			topic: Some("t".into()),
			options: vec![
				EngineOption { id: correct_id, option_text: "right".into(), is_correct: true },
				EngineOption { id: correct_id + 1, option_text: "wrong".into(), is_correct: false },
			],
		}
	}

	#[test]
	fn grades_correct_mcq() {
		let q = mcq(1, 5, 3, 10);
		let s = AnswerSubmission { question_id: 1, selected_option_id: Some(10) };
		assert!(grade_answer(&q, &s).is_correct);
		assert_eq!(grade_answer(&q, &s).marks_awarded, 5);
	}

	#[test]
	fn grades_wrong_mcq() {
		let q = mcq(1, 5, 3, 10);
		let s = AnswerSubmission { question_id: 1, selected_option_id: Some(11) };
		assert!(!grade_answer(&q, &s).is_correct);
	}

	#[test]
	fn attempt_totals() {
		let qs = vec![mcq(1, 5, 2, 10), mcq(2, 5, 4, 20)];
		let sub = AttemptSubmission {
			answers: vec![
				AnswerSubmission { question_id: 1, selected_option_id: Some(10) },
				AnswerSubmission { question_id: 2, selected_option_id: Some(21) },
			],
		};
		let r = grade_attempt(&qs, &sub);
		assert_eq!(r.score, 5);
		assert_eq!(r.total_marks, 10);
		assert_eq!(r.percentage, 50.0);
	}

	#[test]
	fn proficiency_moves_toward_target() {
		let up = update_proficiency(0.5, true, 0.3);
		let down = update_proficiency(0.5, false, 0.3);
		assert!(up > 0.5 && up <= 1.0);
		assert!(down < 0.5 && down >= 0.0);
	}

	#[test]
	fn difficulty_tracks_proficiency() {
		assert!(select_next_difficulty(0.0, 0) <= select_next_difficulty(1.0, 0));
		assert!(select_next_difficulty(0.5, 2) >= select_next_difficulty(0.5, -2));
	}

	#[test]
	fn picks_closest_difficulty() {
		let pool = vec![mcq(1, 1, 1, 1), mcq(2, 1, 5, 1)];
		assert_eq!(pick_adaptive_index(&pool, 5), Some(1));
		assert_eq!(pick_adaptive_index(&pool, 1), Some(0));
	}
}
