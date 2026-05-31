use actix_session::Session;
use actix_web::{web, HttpResponse, Responder};
use tera::{Context, Tera};
use sqlx::PgPool;

use crate::auth::UserRole;

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
	let student_id_opt = sqlx::query_scalar::<_, i32>(
		"SELECT id FROM students WHERE user_id = $1 LIMIT 1",
	)
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
	ctx.insert("current_trimester", "2025/26 Trimester 3");
	ctx.insert("current_date", "Monday, 25 May 2026");

	// TODO: Replace with DB query: SELECT COUNT(*) FROM enrollments(?) WHERE student_id = ?
	ctx.insert("enrolled_course_count", &3);
	ctx.insert("avg_grade", &78);
	ctx.insert("attendance_pct", &91);
	ctx.insert("upcoming_deadlines", &2);

	// Sidebar active page highlight
	ctx.insert("active_page", "dashboard");

	let courses: Vec<crate::CourseContext> = vec![
		crate::CourseContext {
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
		crate::CourseContext {
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
		crate::CourseContext {
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

	let announcements: Vec<crate::AnnouncementContext> = vec![
		crate::AnnouncementContext {
			title: "Assignment 2 brief released".into(),
			course: "CSC1106 – Web Programming".into(),
			date: "24 May 2026".into(),
		},
	];
	ctx.insert("announcements", &announcements);

	let due_dates: Vec<crate::DueDateContext> = vec![
		crate::DueDateContext {
			title: "Assignment 2 Submission".into(),
			course: "CSC1106".into(),
			item_type: "assignment".into(),
			due_date: "28 May".into(),
			urgent: true,
		},
		crate::DueDateContext {
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

pub async fn student_courses(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
	let user = match crate::auth::require_role(&session, UserRole::Student) {
		Ok(user) => user,
		Err(response) => return response,
	};

	let mut ctx = Context::new();
	crate::insert_student_base(&mut ctx, &user.display_name, "2501129");
	ctx.insert("active_page", "courses");
	ctx.insert("current_trimester", "2025/26 Trimester 3");

	// TODO: replace with DB query: SELECT * FROM courses JOIN enrollments WHERE student_id = ?
	let courses: Vec<crate::CourseContext> = vec![
		crate::CourseContext {
			id: 1, code: "CSC1106".into(), name: "Web Programming".into(),
			trimester: "2025/26 Trimester 3".into(), image_url: "".into(),
			pinned: true, ongoing: true, progress: 65,
			lecturer: "Dr. Tan Wei Ming".into(), attendance_pct: 90,
		},
		crate::CourseContext {
			id: 2, code: "CSC1107".into(), name: "Operating Systems".into(),
			trimester: "2025/26 Trimester 3".into(), image_url: "".into(),
			pinned: false, ongoing: true, progress: 50,
			lecturer: "Prof. Lim Ah Kow".into(), attendance_pct: 85,
		},
		crate::CourseContext {
			id: 3, code: "INF2003".into(), name: "Database Systems".into(),
			trimester: "2025/26 Trimester 3".into(), image_url: "".into(),
			pinned: false, ongoing: true, progress: 72,
			lecturer: "Dr. Ng Siew Lin".into(), attendance_pct: 95,
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

pub async fn student_assignments(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
	let user = match crate::auth::require_role(&session, UserRole::Student) {
		Ok(user) => user,
		Err(response) => return response,
	};

	let mut ctx = Context::new();
	crate::insert_student_base(&mut ctx, &user.display_name, "2501129");
	ctx.insert("active_page", "assignments");

	// TODO: replace with DB query for enrolled courses (used by filter dropdown)
	let courses: Vec<crate::CourseContext> = vec![
		crate::CourseContext {
			id: 1, code: "CSC1106".into(), name: "Web Programming".into(),
			trimester: "2025/26 Trimester 3".into(), image_url: "".into(),
			pinned: false, ongoing: true, progress: 65,
			lecturer: "Dr. Tan Wei Ming".into(), attendance_pct: 90,
		},
		crate::CourseContext {
			id: 2, code: "CSC1107".into(), name: "Operating Systems".into(),
			trimester: "2025/26 Trimester 3".into(), image_url: "".into(),
			pinned: false, ongoing: true, progress: 50,
			lecturer: "Prof. Lim Ah Kow".into(), attendance_pct: 85,
		},
		crate::CourseContext {
			id: 3, code: "INF2003".into(), name: "Database Systems".into(),
			trimester: "2025/26 Trimester 3".into(), image_url: "".into(),
			pinned: false, ongoing: true, progress: 72,
			lecturer: "Dr. Ng Siew Lin".into(), attendance_pct: 95,
		},
	];
	ctx.insert("courses", &courses);

	// TODO: replace with DB query: SELECT * FROM assignments/quizzes WHERE student_id = ?
	let assignments: Vec<crate::AssignmentContext> = vec![
		crate::AssignmentContext {
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
		crate::AssignmentContext {
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
		crate::AssignmentContext {
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
		crate::AssignmentContext {
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

pub async fn student_grades(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
	let user = match crate::auth::require_role(&session, UserRole::Student) {
		Ok(user) => user,
		Err(response) => return response,
	};

	let mut ctx = Context::new();
	crate::insert_student_base(&mut ctx, &user.display_name, "2501129");
	ctx.insert("active_page", "grades");

	// TODO: replace with DB queries for actual grade data
	let course_grades: Vec<crate::CourseGradeContext> = vec![
		crate::CourseGradeContext {
			code: "CSC1106".into(),
			name: "Web Programming".into(),
			overall: 82.0,
			grade_letter: "A-".into(),
			items: vec![
				crate::GradeItemContext { title: "Assignment 1".into(), item_type: "assignment".into(), score: 82.0, max_score: 100.0, weight: 20.0 },
				crate::GradeItemContext { title: "Quiz 1".into(),       item_type: "quiz".into(),       score: 18.0, max_score: 20.0,  weight: 10.0 },
				crate::GradeItemContext { title: "Midterm Exam".into(), item_type: "exam".into(),       score: 38.0, max_score: 50.0,  weight: 30.0 },
			],
		},
		crate::CourseGradeContext {
			code: "CSC1107".into(),
			name: "Operating Systems".into(),
			overall: 74.0,
			grade_letter: "B".into(),
			items: vec![
				crate::GradeItemContext { title: "Assignment 1".into(), item_type: "assignment".into(), score: 75.0, max_score: 100.0, weight: 20.0 },
				crate::GradeItemContext { title: "Quiz 2".into(),       item_type: "quiz".into(),       score: 14.0, max_score: 20.0,  weight: 10.0 },
				crate::GradeItemContext { title: "Midterm Exam".into(), item_type: "exam".into(),       score: 35.0, max_score: 50.0,  weight: 30.0 },
			],
		},
		crate::CourseGradeContext {
			code: "INF2003".into(),
			name: "Database Systems".into(),
			overall: 89.0,
			grade_letter: "A".into(),
			items: vec![
				crate::GradeItemContext { title: "Lab Report 1".into(), item_type: "assignment".into(), score: 90.0, max_score: 100.0, weight: 20.0 },
				crate::GradeItemContext { title: "Quiz 1".into(),        item_type: "quiz".into(),      score: 19.0, max_score: 20.0,  weight: 10.0 },
				crate::GradeItemContext { title: "Midterm Exam".into(),  item_type: "exam".into(),      score: 44.0, max_score: 50.0,  weight: 30.0 },
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

pub async fn student_announcement(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
	let user = match crate::auth::require_role(&session, UserRole::Student) {
		Ok(user) => user,
		Err(response) => return response,
	};

	let mut ctx = Context::new();
	crate::insert_student_base(&mut ctx, &user.display_name, "2501129");
	ctx.insert("active_page", "announcements");

	// TODO: replace with DB query for enrolled courses (used by filter dropdown)
	let courses: Vec<crate::CourseContext> = vec![
		crate::CourseContext { id: 1, code: "CSC1106".into(), name: "Web Programming".into(), trimester: "2025/26 Trimester 3".into(), image_url: "".into(), pinned: false, ongoing: true, progress: 65, lecturer: "Dr. Tan Wei Ming".into(), attendance_pct: 90 },
		crate::CourseContext { id: 2, code: "CSC1107".into(), name: "Operating Systems".into(), trimester: "2025/26 Trimester 3".into(), image_url: "".into(), pinned: false, ongoing: true, progress: 50, lecturer: "Prof. Lim Ah Kow".into(), attendance_pct: 85 },
		crate::CourseContext { id: 3, code: "INF2003".into(), name: "Database Systems".into(), trimester: "2025/26 Trimester 3".into(), image_url: "".into(), pinned: false, ongoing: true, progress: 72, lecturer: "Dr. Ng Siew Lin".into(), attendance_pct: 95 },
	];
	ctx.insert("courses", &courses);

	// TODO: replace with DB query: SELECT * FROM announcements WHERE course_id IN (enrolled) ORDER BY date DESC
	let announcements: Vec<crate::AnnouncementFullContext> = vec![
		crate::AnnouncementFullContext { id: 1, title: "Assignment 2 brief released".into(), course: "Web Programming".into(), course_code: "CSC1106".into(), date: "24 May 2026".into(), content: "The brief for Assignment 2 has been uploaded to the course portal. Please review the requirements and submit by 28 May.".into(), is_new: true },
		crate::AnnouncementFullContext { id: 2, title: "Midterm rescheduled to Week 8".into(), course: "Operating Systems".into(), course_code: "CSC1107".into(), date: "22 May 2026".into(), content: "Due to the public holiday, the midterm exam has been moved to Week 8. New date: 5 June 2026 at 10am.".into(), is_new: true },
		crate::AnnouncementFullContext { id: 3, title: "Lab session cancelled this Friday".into(), course: "Database Systems".into(), course_code: "INF2003".into(), date: "20 May 2026".into(), content: "The lab session scheduled for Friday 23 May is cancelled. A replacement session will be arranged.".into(), is_new: false },
	];
	ctx.insert("announcements", &announcements);

	let rendered = match tmpl.render("student/announcement.html", &ctx) {
		Ok(html) => html,
		Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
	};
	HttpResponse::Ok().content_type("text/html").body(rendered)
}

pub async fn student_quiz(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
	let user = match crate::auth::require_role(&session, UserRole::Student) {
		Ok(user) => user,
		Err(response) => return response,
	};

	let mut ctx = Context::new();
	crate::insert_student_base(&mut ctx, &user.display_name, "2501129");
	ctx.insert("active_page", "quizzes");

	// TODO: replace with DB query for enrolled courses (used by filter dropdown)
	let courses: Vec<crate::CourseContext> = vec![
		crate::CourseContext { id: 1, code: "CSC1106".into(), name: "Web Programming".into(), trimester: "2025/26 Trimester 3".into(), image_url: "".into(), pinned: false, ongoing: true, progress: 65, lecturer: "Dr. Tan Wei Ming".into(), attendance_pct: 90 },
		crate::CourseContext { id: 2, code: "CSC1107".into(), name: "Operating Systems".into(), trimester: "2025/26 Trimester 3".into(), image_url: "".into(), pinned: false, ongoing: true, progress: 50, lecturer: "Prof. Lim Ah Kow".into(), attendance_pct: 85 },
		crate::CourseContext { id: 3, code: "INF2003".into(), name: "Database Systems".into(), trimester: "2025/26 Trimester 3".into(), image_url: "".into(), pinned: false, ongoing: true, progress: 72, lecturer: "Dr. Ng Siew Lin".into(), attendance_pct: 95 },
	];
	ctx.insert("courses", &courses);

	// TODO: replace with DB query: SELECT * FROM quizzes JOIN enrollments WHERE student_id = ?
	let quizzes: Vec<crate::QuizContext> = vec![
		crate::QuizContext { id: 1, title: "Quiz 1 – HTML & CSS Basics".into(), course_code: "CSC1106".into(), course_name: "Web Programming".into(), due_date: "10 Apr 2026".into(), duration_mins: 20, status: "completed".into(), score: Some("18 / 20".into()), total_marks: 20, attempt_allowed: 1, attempts_used: 1, urgent: false },
		crate::QuizContext { id: 2, title: "Quiz 2 – JavaScript Fundamentals".into(), course_code: "CSC1106".into(), course_name: "Web Programming".into(), due_date: "28 May 2026".into(), duration_mins: 25, status: "open".into(), score: None, total_marks: 25, attempt_allowed: 2, attempts_used: 0, urgent: true },
		crate::QuizContext { id: 3, title: "Quiz 3 – Process Scheduling".into(), course_code: "CSC1107".into(), course_name: "Operating Systems".into(), due_date: "30 May 2026".into(), duration_mins: 30, status: "upcoming".into(), score: None, total_marks: 30, attempt_allowed: 1, attempts_used: 0, urgent: false },
		crate::QuizContext { id: 4, title: "Quiz 1 – Relational Model".into(), course_code: "INF2003".into(), course_name: "Database Systems".into(), due_date: "5 Apr 2026".into(), duration_mins: 20, status: "completed".into(), score: Some("19 / 20".into()), total_marks: 20, attempt_allowed: 1, attempts_used: 1, urgent: false },
		crate::QuizContext { id: 5, title: "Quiz 2 – Memory Management".into(), course_code: "CSC1107".into(), course_name: "Operating Systems".into(), due_date: "2 Apr 2026".into(), duration_mins: 20, status: "missed".into(), score: None, total_marks: 20, attempt_allowed: 1, attempts_used: 0, urgent: false },
	];
	// Pre-compute stat-card counts (Tera doesn't support "in" or | list filters)
	let quiz_open_count = quizzes.iter().filter(|q| q.status == "open").count();
	let quiz_upcoming_count = quizzes.iter().filter(|q| q.status == "upcoming" || q.status == "open").count();
	let quiz_completed_count = quizzes.iter().filter(|q| q.status == "completed").count();
	let quiz_missed_count = quizzes.iter().filter(|q| q.status == "missed").count();

	ctx.insert("quizzes", &quizzes);
	ctx.insert("quiz_open_count", &quiz_open_count);
	ctx.insert("quiz_upcoming_count", &quiz_upcoming_count);
	ctx.insert("quiz_completed_count", &quiz_completed_count);
	ctx.insert("quiz_missed_count", &quiz_missed_count);

	let rendered = match tmpl.render("student/quiz.html", &ctx) {
		Ok(html) => html,
		Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
	};
	HttpResponse::Ok().content_type("text/html").body(rendered)
}

pub async fn student_quiz_attempt(
	path: web::Path<i32>,
	tmpl: web::Data<Tera>,
	session: Session,
) -> impl Responder {
	let user = match crate::auth::require_role(&session, UserRole::Student) {
		Ok(user) => user,
		Err(response) => return response,
	};

	let quiz_id = path.into_inner();
	let Some(quiz) = crate::mock_quiz_attempt(quiz_id) else {
		return HttpResponse::NotFound().content_type("text/plain").body("Quiz attempt is not available.");
	};

	if let Err(error) = session.insert(crate::quiz_monitoring_ready_key(quiz_id), false) {
		return HttpResponse::InternalServerError().body(format!("Failed to reset quiz monitoring session: {error}"));
	}

	let mut ctx = Context::new();
	crate::insert_student_base(&mut ctx, &user.display_name, "2501129");
	ctx.insert("active_page", "quizzes");
	ctx.insert("quiz", &quiz);
	ctx.insert("monitoring_event_url", &format!("/student/quizzes/{quiz_id}/monitoring-events"));
	ctx.insert("monitoring_ready_url", &format!("/student/quizzes/{quiz_id}/monitoring-ready"));
	ctx.insert("quiz_start_url", &format!("/student/quizzes/{quiz_id}/take"));

	let rendered = match tmpl.render("student/quiz_attempt.html", &ctx) {
		Ok(html) => html,
		Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
	};

	HttpResponse::Ok().content_type("text/html").body(rendered)
}

pub async fn student_quiz_take(
	path: web::Path<i32>,
	tmpl: web::Data<Tera>,
	session: Session,
) -> impl Responder {
	let user = match crate::auth::require_role(&session, UserRole::Student) {
		Ok(user) => user,
		Err(response) => return response,
	};

	let quiz_id = path.into_inner();
	let Some(quiz) = crate::mock_quiz_attempt(quiz_id) else {
		return HttpResponse::NotFound().content_type("text/plain").body("Quiz attempt is not available.");
	};

	match crate::quiz_monitoring_ready(&session, quiz_id) {
		Ok(true) => {}
		Ok(false) => {
			return HttpResponse::SeeOther().insert_header(("Location", format!("/student/quizzes/{quiz_id}/attempt"))).finish();
		}
		Err(response) => return response,
	}

	let mut ctx = Context::new();
	crate::insert_student_base(&mut ctx, &user.display_name, "2501129");
	ctx.insert("active_page", "quizzes");
	ctx.insert("quiz", &quiz);
	ctx.insert("questions", &crate::mock_quiz_questions());
	ctx.insert("quiz_seconds", &(quiz.duration_mins * 60));
	ctx.insert("monitoring_event_url", &format!("/student/quizzes/{quiz_id}/monitoring-events"));
	ctx.insert("monitoring_ready_url", &format!("/student/quizzes/{quiz_id}/monitoring-ready"));

	let rendered = match tmpl.render("student/quiz_take.html", &ctx) {
		Ok(html) => html,
		Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
	};

	HttpResponse::Ok().content_type("text/html").body(rendered)
}

pub async fn mark_quiz_monitoring_ready(
	path: web::Path<i32>,
	session: Session,
) -> impl Responder {
	let _user = match crate::auth::require_role(&session, UserRole::Student) {
		Ok(user) => user,
		Err(response) => return response,
	};

	let quiz_id = path.into_inner();
	if crate::mock_quiz_attempt(quiz_id).is_none() {
		return HttpResponse::NotFound().content_type("text/plain").body("Quiz attempt is not available.");
	}

	match session.insert(crate::quiz_monitoring_ready_key(quiz_id), true) {
		Ok(_) => HttpResponse::Ok().json(crate::QuizMonitoringEventResponse { status: "ready" }),
		Err(error) => HttpResponse::InternalServerError().body(format!("Failed to mark monitoring ready: {error}")),
	}
}

pub async fn save_quiz_monitoring_event(
	path: web::Path<i32>,
	db: web::Data<PgPool>,
	session: Session,
	payload: web::Json<crate::QuizMonitoringEventPayload>,
) -> impl Responder {
	let user = match crate::auth::require_role(&session, UserRole::Student) {
		Ok(user) => user,
		Err(response) => return response,
	};

	let quiz_id = path.into_inner();
	if crate::mock_quiz_attempt(quiz_id).is_none() {
		return HttpResponse::NotFound().content_type("text/plain").body("Quiz attempt is not available.");
	}

	let event_type = payload.event_type.trim().to_lowercase();
	let severity = payload.severity.trim().to_lowercase();

	if !crate::valid_monitoring_event_type(&event_type) {
		return HttpResponse::BadRequest().content_type("text/plain").body("Unknown monitoring event type.");
	}

	if !crate::valid_monitoring_severity(&severity) {
		return HttpResponse::BadRequest().content_type("text/plain").body("Unknown monitoring event severity.");
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
		Err(error) => HttpResponse::InternalServerError().body(format!("Failed to save event: {error}")),
	}
}

pub async fn student_attendance(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
	let user = match crate::auth::require_role(&session, UserRole::Student) {
		Ok(user) => user,
		Err(response) => return response,
	};

	let mut ctx = Context::new();
	crate::insert_student_base(&mut ctx, &user.display_name, "2501129");
	ctx.insert("active_page", "attendance");

	// TODO: replace with DB query for enrolled courses (filter dropdown)
	let courses: Vec<crate::CourseContext> = vec![
		crate::CourseContext { id: 1, code: "CSC1106".into(), name: "Web Programming".into(), trimester: "2025/26 Trimester 3".into(), image_url: "".into(), pinned: false, ongoing: true, progress: 65, lecturer: "Dr. Tan Wei Ming".into(), attendance_pct: 90 },
		crate::CourseContext { id: 2, code: "CSC1107".into(), name: "Operating Systems".into(), trimester: "2025/26 Trimester 3".into(), image_url: "".into(), pinned: false, ongoing: true, progress: 50, lecturer: "Prof. Lim Ah Kow".into(), attendance_pct: 85 },
		crate::CourseContext { id: 3, code: "INF2003".into(), name: "Database Systems".into(), trimester: "2025/26 Trimester 3".into(), image_url: "".into(), pinned: false, ongoing: true, progress: 72, lecturer: "Dr. Ng Siew Lin".into(), attendance_pct: 95 },
	];
	ctx.insert("courses", &courses);

	// TODO: replace with DB query: SELECT sessions, attendance_records WHERE student_id = ?
	let attendance_courses: Vec<crate::AttendanceCourseContext> = vec![
		crate::AttendanceCourseContext { code: "CSC1106".into(), name: "Web Programming".into(), pct: 90, attended: 9, total: 10, sessions: vec![crate::AttendanceSessionContext { date: "5 Mar 2026".into(),  topic: "Introduction to HTML".into(),     status: "present".into() }, crate::AttendanceSessionContext { date: "12 Mar 2026".into(), topic: "CSS Layouts & Flexbox".into(),    status: "present".into() }, crate::AttendanceSessionContext { date: "19 Mar 2026".into(), topic: "JavaScript Basics".into(),        status: "present".into() }, crate::AttendanceSessionContext { date: "26 Mar 2026".into(), topic: "DOM Manipulation".into(),         status: "absent".into()  }, crate::AttendanceSessionContext { date: "2 Apr 2026".into(),  topic: "Fetch API & AJAX".into(),         status: "present".into() }, crate::AttendanceSessionContext { date: "9 Apr 2026".into(),  topic: "Forms & Validation".into(),       status: "present".into() }, crate::AttendanceSessionContext { date: "16 Apr 2026".into(), topic: "Responsive Design".into(),        status: "present".into() }, crate::AttendanceSessionContext { date: "23 Apr 2026".into(), topic: "Frameworks Overview".into(),      status: "present".into() }, crate::AttendanceSessionContext { date: "7 May 2026".into(),  topic: "Backend Integration".into(),      status: "present".into() }, crate::AttendanceSessionContext { date: "14 May 2026".into(), topic: "Project Workshop".into(),         status: "present".into() }] },
		crate::AttendanceCourseContext { code: "CSC1107".into(), name: "Operating Systems".into(), pct: 85, attended: 11, total: 13, sessions: vec![crate::AttendanceSessionContext { date: "4 Mar 2026".into(),  topic: "OS Overview".into(),              status: "present".into() }, crate::AttendanceSessionContext { date: "11 Mar 2026".into(), topic: "Process Management".into(),       status: "present".into() }, crate::AttendanceSessionContext { date: "18 Mar 2026".into(), topic: "CPU Scheduling".into(),           status: "late".into()    }, crate::AttendanceSessionContext { date: "25 Mar 2026".into(), topic: "Deadlocks".into(),                status: "present".into() }, crate::AttendanceSessionContext { date: "1 Apr 2026".into(),  topic: "Memory Management".into(),        status: "absent".into()  }, crate::AttendanceSessionContext { date: "8 Apr 2026".into(),  topic: "Virtual Memory".into(),           status: "present".into() }, crate::AttendanceSessionContext { date: "15 Apr 2026".into(), topic: "File Systems".into(),             status: "present".into() }, crate::AttendanceSessionContext { date: "22 Apr 2026".into(), topic: "I/O Systems".into(),              status: "absent".into()  }, crate::AttendanceSessionContext { date: "6 May 2026".into(),  topic: "Security Basics".into(),          status: "present".into() }, crate::AttendanceSessionContext { date: "13 May 2026".into(), topic: "Virtualisation".into(),           status: "present".into() }, crate::AttendanceSessionContext { date: "20 May 2026".into(), topic: "Cloud OS Concepts".into(),        status: "present".into() }, crate::AttendanceSessionContext { date: "22 May 2026".into(), topic: "Revision Session".into(),         status: "present".into() }, crate::AttendanceSessionContext { date: "27 May 2026".into(), topic: "Exam Prep Q&A".into(),            status: "present".into() }] },
		crate::AttendanceCourseContext { code: "INF2003".into(), name: "Database Systems".into(), pct: 95, attended: 10, total: 11, sessions: vec![crate::AttendanceSessionContext { date: "6 Mar 2026".into(),  topic: "Relational Model".into(),         status: "present".into() }, crate::AttendanceSessionContext { date: "13 Mar 2026".into(), topic: "SQL Basics".into(),               status: "present".into() }, crate::AttendanceSessionContext { date: "20 Mar 2026".into(), topic: "Advanced SQL".into(),             status: "present".into() }, crate::AttendanceSessionContext { date: "27 Mar 2026".into(), topic: "Normalisation".into(),            status: "present".into() }, crate::AttendanceSessionContext { date: "3 Apr 2026".into(),  topic: "ER Diagrams".into(),              status: "present".into() }, crate::AttendanceSessionContext { date: "10 Apr 2026".into(), topic: "Transactions & ACID".into(),      status: "present".into() }, crate::AttendanceSessionContext { date: "17 Apr 2026".into(), topic: "Indexing & Performance".into(),   status: "excused".into() }, crate::AttendanceSessionContext { date: "24 Apr 2026".into(), topic: "NoSQL Overview".into(),           status: "present".into() }, crate::AttendanceSessionContext { date: "8 May 2026".into(),  topic: "Database Security".into(),        status: "present".into() }, crate::AttendanceSessionContext { date: "15 May 2026".into(), topic: "Lab: Schema Design".into(),       status: "present".into() }, crate::AttendanceSessionContext { date: "22 May 2026".into(), topic: "Project Consultation".into(),     status: "present".into() }] },
	];

	// Derive overall stats from the course data
	let total_sessions: i32     = attendance_courses.iter().map(|c| c.total).sum();
	let attended_sessions: i32  = attendance_courses.iter().map(|c| c.attended).sum();
	let absent_sessions: i32    = attendance_courses.iter().flat_map(|c| &c.sessions).filter(|s| s.status == "absent").count() as i32;
	let overall_pct: i32 = if total_sessions > 0 { (attended_sessions * 100) / total_sessions } else { 0 };

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

pub async fn student_forum(tmpl: web::Data<Tera>, session: Session) -> impl Responder {
	let user = match crate::auth::require_role(&session, UserRole::Student) {
		Ok(user) => user,
		Err(response) => return response,
	};

	let mut ctx = Context::new();
	crate::insert_student_base(&mut ctx, &user.display_name, "2501129");
	ctx.insert("active_page", "forum");

	// TODO: replace with DB query for enrolled courses (filter dropdown)
	let courses: Vec<crate::CourseContext> = vec![
		crate::CourseContext { id: 1, code: "CSC1106".into(), name: "Web Programming".into(), trimester: "2025/26 Trimester 3".into(), image_url: "".into(), pinned: false, ongoing: true, progress: 65, lecturer: "Dr. Tan Wei Ming".into(), attendance_pct: 90 },
		crate::CourseContext { id: 2, code: "CSC1107".into(), name: "Operating Systems".into(), trimester: "2025/26 Trimester 3".into(), image_url: "".into(), pinned: false, ongoing: true, progress: 50, lecturer: "Prof. Lim Ah Kow".into(), attendance_pct: 85 },
		crate::CourseContext { id: 3, code: "INF2003".into(), name: "Database Systems".into(), trimester: "2025/26 Trimester 3".into(), image_url: "".into(), pinned: false, ongoing: true, progress: 72, lecturer: "Dr. Ng Siew Lin".into(), attendance_pct: 95 },
	];
	ctx.insert("courses", &courses);

	// TODO: replace with DB query: SELECT * FROM forum_threads WHERE course_id IN (enrolled) ORDER BY last_reply_at DESC
	let threads: Vec<crate::ThreadContext> = vec![
		crate::ThreadContext { id: 1, title: "How do I centre a div vertically in CSS?".into(), course_code: "CSC1106".into(), course_name: "Web Programming".into(), author: "Lee Zhi Yu".into(), author_initials: "LZ".into(), created_at: "20 May 2026".into(), last_reply_at: "24 May 2026".into(), reply_count: 5, view_count: 42, is_pinned: false, is_answered: true, is_mine: true, tags: vec!["css".into(), "question".into()], preview: "I've been trying to vertically centre a div inside a full-height container but flexbox doesn't seem to work as expected. Any tips?".into() },
		crate::ThreadContext { id: 2, title: "[PINNED] Assignment 2 – Clarifications & FAQ".into(), course_code: "CSC1106".into(), course_name: "Web Programming".into(), author: "Dr. Tan Wei Ming".into(), author_initials: "TW".into(), created_at: "24 May 2026".into(), last_reply_at: "25 May 2026".into(), reply_count: 12, view_count: 198, is_pinned: true, is_answered: false, is_mine: false, tags: vec!["announcement".into(), "assignment".into()], preview: "This thread collects all common questions about Assignment 2. Please read before posting a new question. Submission deadline: 28 May 2026.".into() },
		crate::ThreadContext { id: 3, title: "Confused about the difference between paging and segmentation".into(), course_code: "CSC1107".into(), course_name: "Operating Systems".into(), author: "Aisha Rahman".into(), author_initials: "AR".into(), created_at: "22 May 2026".into(), last_reply_at: "23 May 2026".into(), reply_count: 3, view_count: 56, is_pinned: false, is_answered: false, is_mine: false, tags: vec!["os".into(), "question".into()], preview: "I'm trying to understand paging vs segmentation...".into() },
	];
	ctx.insert("threads", &threads);

	let rendered = match tmpl.render("student/discussionforum.html", &ctx) {
		Ok(html) => html,
		Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
	};
	HttpResponse::Ok().content_type("text/html").body(rendered)
}
