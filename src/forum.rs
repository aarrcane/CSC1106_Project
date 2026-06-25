use actix_session::Session;
use actix_multipart::Multipart;
use actix_web::{HttpResponse, http::header, web};
use futures_util::TryStreamExt as _;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use std::time::{SystemTime, UNIX_EPOCH};
use tera::{Context, Tera};

use crate::auth::UserRole;

const MAX_ATTACHMENTS_PER_ITEM: usize = 3;
const MAX_ATTACHMENT_BYTES: usize = 5 * 1024 * 1024;

#[derive(Deserialize)]
pub struct DeleteForm {
    reason: Option<String>,
}

#[derive(Deserialize)]
pub struct ModerationForm {
    reason: Option<String>,
}

#[derive(Serialize, FromRow)]
struct CourseOption {
    id: i32,
    code: String,
    name: String,
}

#[derive(Serialize, FromRow, Clone)]
struct ForumAttachmentView {
    id: i32,
    original_filename: String,
    content_type: String,
    file_size: i32,
    public_url: String,
}

struct PendingUpload {
    filename: String,
    content_type: String,
    bytes: Vec<u8>,
}

#[derive(Default)]
struct ForumMultipartData {
    course_id: Option<i32>,
    title: Option<String>,
    body: Option<String>,
    tags: Option<String>,
    thread_type: Option<String>,
    parent_post_id: Option<i32>,
    images: Vec<PendingUpload>,
}

#[derive(Serialize, FromRow)]
struct ForumThreadListItem {
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
    is_locked: bool,
    is_announcement: bool,
    is_mine: bool,
    tags: Vec<String>,
    preview: String,
}

#[derive(Serialize, FromRow)]
struct ForumThreadDetail {
    id: i32,
    course_id: i32,
    course_code: String,
    course_name: String,
    created_by: i32,
    title: String,
    body: String,
    tags: Vec<String>,
    author: String,
    author_initials: String,
    created_at: String,
    updated_at: String,
    edited_at: Option<String>,
    deleted_at: Option<String>,
    deleted_by_name: Option<String>,
    delete_reason: Option<String>,
    locked_at: Option<String>,
    locked_by_name: Option<String>,
    is_pinned: bool,
    is_answered: bool,
    is_announcement: bool,
    view_count: i32,
    is_mine: bool,
    attachments: Vec<ForumAttachmentView>,
}

#[derive(Serialize, FromRow, Clone)]
struct ForumPostFlat {
    id: i32,
    thread_id: i32,
    user_id: i32,
    parent_post_id: Option<i32>,
    body: String,
    author: String,
    author_initials: String,
    created_at: String,
    updated_at: String,
    edited_at: Option<String>,
    deleted_at: Option<String>,
    deleted_by_name: Option<String>,
    delete_reason: Option<String>,
    is_mine: bool,
}

#[derive(Serialize)]
struct ForumPostView {
    id: i32,
    thread_id: i32,
    user_id: i32,
    parent_post_id: Option<i32>,
    body: String,
    author: String,
    author_initials: String,
    created_at: String,
    updated_at: String,
    edited_at: Option<String>,
    deleted_at: Option<String>,
    deleted_by_name: Option<String>,
    delete_reason: Option<String>,
    is_mine: bool,
    depth: i32,
    attachments: Vec<ForumAttachmentView>,
}

#[derive(Serialize, FromRow)]
struct ModerationActionView {
    action: String,
    target_type: String,
    reason: Option<String>,
    moderator: String,
    created_at: String,
}

pub async fn student_forum(
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
) -> HttpResponse {
    let user = match crate::auth::require_role(&session, UserRole::Student) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let Some(student_id) = student_id_for_user(db.get_ref(), user.id).await else {
        return HttpResponse::Forbidden().body("Student profile not found.");
    };

    render_forum_list(
        tmpl.get_ref(),
        db.get_ref(),
        ForumAudience::Student {
            user_id: user.id,
            display_name: &user.display_name,
            student_id,
            course_id: None,
        },
    )
    .await
}

pub async fn student_course_forum(
    path: web::Path<i32>,
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
) -> HttpResponse {
    let user = match crate::auth::require_role(&session, UserRole::Student) {
        Ok(user) => user,
        Err(response) => return response,
    };
    let course_id = path.into_inner();

    let Some(student_id) = student_id_for_user(db.get_ref(), user.id).await else {
        return HttpResponse::Forbidden().body("Student profile not found.");
    };
    if !student_can_access_course(db.get_ref(), student_id, course_id).await {
        return HttpResponse::Forbidden().body("You are not enrolled in this course.");
    }

    render_forum_list(
        tmpl.get_ref(),
        db.get_ref(),
        ForumAudience::Student {
            user_id: user.id,
            display_name: &user.display_name,
            student_id,
            course_id: Some(course_id),
        },
    )
    .await
}

pub async fn lecturer_forum(
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
) -> HttpResponse {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let Some(lecturer_id) = lecturer_id_for_user(db.get_ref(), user.id).await else {
        return HttpResponse::Forbidden().body("Lecturer profile not found.");
    };

    render_forum_list(
        tmpl.get_ref(),
        db.get_ref(),
        ForumAudience::Lecturer {
            user_id: user.id,
            display_name: &user.display_name,
            lecturer_id,
            course_id: None,
        },
    )
    .await
}

pub async fn lecturer_course_forum(
    path: web::Path<i32>,
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    session: Session,
) -> HttpResponse {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };
    let course_id = path.into_inner();

    let Some(lecturer_id) = lecturer_id_for_user(db.get_ref(), user.id).await else {
        return HttpResponse::Forbidden().body("Lecturer profile not found.");
    };
    if !lecturer_owns_course(db.get_ref(), lecturer_id, course_id).await {
        return HttpResponse::Forbidden().body("You do not manage this course.");
    }

    render_forum_list(
        tmpl.get_ref(),
        db.get_ref(),
        ForumAudience::Lecturer {
            user_id: user.id,
            display_name: &user.display_name,
            lecturer_id,
            course_id: Some(course_id),
        },
    )
    .await
}

pub async fn create_student_thread(
    db: web::Data<PgPool>,
    storage: web::Data<crate::storage::SupabaseStorage>,
    session: Session,
    payload: Multipart,
) -> HttpResponse {
    let user = match crate::auth::require_role(&session, UserRole::Student) {
        Ok(user) => user,
        Err(response) => return response,
    };
    let form = match parse_forum_multipart(payload).await {
        Ok(form) => form,
        Err(message) => return redirect_with_message("/student/forum", &message),
    };
    let Some(course_id) = form.course_id else {
        return redirect_with_message("/student/forum", "Course is required.");
    };

    let Some(student_id) = student_id_for_user(db.get_ref(), user.id).await else {
        return HttpResponse::Forbidden().body("Student profile not found.");
    };
    if !student_can_access_course(db.get_ref(), student_id, course_id).await {
        return HttpResponse::Forbidden().body("You are not enrolled in this course.");
    }

    let title = form.title.as_deref().unwrap_or("").trim();
    let body = form.body.as_deref().unwrap_or("").trim();
    if title.is_empty() || body.is_empty() {
        return redirect_with_message("/student/forum", "Title and message are required.");
    }

    let thread_id = match insert_thread(
        db.get_ref(),
        course_id,
        user.id,
        title,
        body,
        form.tags.as_deref(),
        "discussion",
    )
    .await
    {
        Ok(thread_id) => thread_id,
        Err(error) => return HttpResponse::InternalServerError().body(error.to_string()),
    };

    if let Err(message) = save_thread_attachments(
        db.get_ref(),
        storage.get_ref(),
        thread_id,
        user.id,
        form.images,
    )
    .await
    {
        return redirect_with_message(&format!("/student/forum/threads/{thread_id}"), &message);
    }

    notify_course_lecturer(
        db.get_ref(),
        course_id,
        "New forum thread",
        &format!("{} created a discussion thread: {title}", user.display_name),
        &format!("/lecturer/forum/threads/{thread_id}"),
    )
    .await;

    redirect(&format!("/student/forum/threads/{thread_id}"))
}

pub async fn create_lecturer_thread(
    db: web::Data<PgPool>,
    storage: web::Data<crate::storage::SupabaseStorage>,
    session: Session,
    payload: Multipart,
) -> HttpResponse {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };
    let form = match parse_forum_multipart(payload).await {
        Ok(form) => form,
        Err(message) => return redirect_with_message("/lecturer/forum", &message),
    };
    let Some(course_id) = form.course_id else {
        return redirect_with_message("/lecturer/forum", "Course is required.");
    };

    let Some(lecturer_id) = lecturer_id_for_user(db.get_ref(), user.id).await else {
        return HttpResponse::Forbidden().body("Lecturer profile not found.");
    };
    if !lecturer_owns_course(db.get_ref(), lecturer_id, course_id).await {
        return HttpResponse::Forbidden().body("You do not manage this course.");
    }

    let title = form.title.as_deref().unwrap_or("").trim();
    let body = form.body.as_deref().unwrap_or("").trim();
    if title.is_empty() || body.is_empty() {
        return redirect_with_message("/lecturer/forum", "Title and message are required.");
    }

    let thread_type = match form.thread_type.as_deref() {
        Some("announcement") => "announcement",
        _ => "discussion",
    };

    let thread_id = match insert_thread(
        db.get_ref(),
        course_id,
        user.id,
        title,
        body,
        form.tags.as_deref(),
        thread_type,
    )
    .await
    {
        Ok(thread_id) => thread_id,
        Err(error) => return HttpResponse::InternalServerError().body(error.to_string()),
    };

    if let Err(message) = save_thread_attachments(
        db.get_ref(),
        storage.get_ref(),
        thread_id,
        user.id,
        form.images,
    )
    .await
    {
        return redirect_with_message(&format!("/lecturer/forum/threads/{thread_id}"), &message);
    }

    if thread_type == "announcement" {
        notify_enrolled_students(
            db.get_ref(),
            course_id,
            "New forum announcement",
            &format!("{} posted: {title}", user.display_name),
            &format!("/student/forum/threads/{thread_id}"),
        )
        .await;
    }

    redirect(&format!("/lecturer/forum/threads/{thread_id}"))
}

pub async fn student_thread_detail(
    path: web::Path<i32>,
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    storage: web::Data<crate::storage::SupabaseStorage>,
    session: Session,
) -> HttpResponse {
    let user = match crate::auth::require_role(&session, UserRole::Student) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let Some(student_id) = student_id_for_user(db.get_ref(), user.id).await else {
        return HttpResponse::Forbidden().body("Student profile not found.");
    };
    render_thread_detail(
        tmpl.get_ref(),
        db.get_ref(),
        storage.get_ref(),
        ForumAudience::Student {
            user_id: user.id,
            display_name: &user.display_name,
            student_id,
            course_id: None,
        },
        path.into_inner(),
    )
    .await
}

pub async fn lecturer_thread_detail(
    path: web::Path<i32>,
    tmpl: web::Data<Tera>,
    db: web::Data<PgPool>,
    storage: web::Data<crate::storage::SupabaseStorage>,
    session: Session,
) -> HttpResponse {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };

    let Some(lecturer_id) = lecturer_id_for_user(db.get_ref(), user.id).await else {
        return HttpResponse::Forbidden().body("Lecturer profile not found.");
    };
    render_thread_detail(
        tmpl.get_ref(),
        db.get_ref(),
        storage.get_ref(),
        ForumAudience::Lecturer {
            user_id: user.id,
            display_name: &user.display_name,
            lecturer_id,
            course_id: None,
        },
        path.into_inner(),
    )
    .await
}

pub async fn add_student_reply(
    path: web::Path<i32>,
    db: web::Data<PgPool>,
    storage: web::Data<crate::storage::SupabaseStorage>,
    session: Session,
    payload: Multipart,
) -> HttpResponse {
    let user = match crate::auth::require_role(&session, UserRole::Student) {
        Ok(user) => user,
        Err(response) => return response,
    };
    let thread_id = path.into_inner();
    let form = match parse_forum_multipart(payload).await {
        Ok(form) => form,
        Err(message) => {
            return redirect_with_message(&format!("/student/forum/threads/{thread_id}"), &message);
        }
    };

    let Some(student_id) = student_id_for_user(db.get_ref(), user.id).await else {
        return HttpResponse::Forbidden().body("Student profile not found.");
    };
    let Some(thread) = load_thread(db.get_ref(), thread_id, user.id).await else {
        return HttpResponse::NotFound().body("Thread not found.");
    };
    if !student_can_access_course(db.get_ref(), student_id, thread.course_id).await {
        return HttpResponse::Forbidden().body("You are not enrolled in this course.");
    }
    if thread.locked_at.is_some() || thread.deleted_at.is_some() {
        return HttpResponse::Forbidden().body("This thread is closed.");
    }

    let body = form.body.as_deref().unwrap_or("").trim();
    if body.is_empty() {
        return redirect_with_message(
            &format!("/student/forum/threads/{thread_id}"),
            "Reply cannot be empty.",
        );
    }
    if let Some(parent_id) = form.parent_post_id {
        if !post_belongs_to_thread(db.get_ref(), parent_id, thread_id).await {
            return HttpResponse::BadRequest().body("Parent reply does not belong to this thread.");
        }
    }

    let post_id = match insert_reply(db.get_ref(), thread_id, user.id, form.parent_post_id, body).await {
        Ok(post_id) => post_id,
        Err(error) => return HttpResponse::InternalServerError().body(error.to_string()),
    };

    if let Err(message) =
        save_post_attachments(db.get_ref(), storage.get_ref(), post_id, user.id, form.images).await
    {
        return redirect_with_message(&format!("/student/forum/threads/{thread_id}"), &message);
    }

    if thread.created_by != user.id {
        insert_notification(
            db.get_ref(),
            thread.created_by,
            "New forum reply",
            &format!(
                "{} replied to your thread: {}",
                user.display_name, thread.title
            ),
            &format!("/student/forum/threads/{thread_id}"),
        )
        .await;
    }

    redirect(&format!("/student/forum/threads/{thread_id}"))
}

pub async fn add_lecturer_reply(
    path: web::Path<i32>,
    db: web::Data<PgPool>,
    storage: web::Data<crate::storage::SupabaseStorage>,
    session: Session,
    payload: Multipart,
) -> HttpResponse {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };
    let thread_id = path.into_inner();
    let form = match parse_forum_multipart(payload).await {
        Ok(form) => form,
        Err(message) => {
            return redirect_with_message(&format!("/lecturer/forum/threads/{thread_id}"), &message);
        }
    };

    let Some(lecturer_id) = lecturer_id_for_user(db.get_ref(), user.id).await else {
        return HttpResponse::Forbidden().body("Lecturer profile not found.");
    };
    let Some(thread) = load_thread(db.get_ref(), thread_id, user.id).await else {
        return HttpResponse::NotFound().body("Thread not found.");
    };
    if !lecturer_owns_course(db.get_ref(), lecturer_id, thread.course_id).await {
        return HttpResponse::Forbidden().body("You do not manage this course.");
    }
    if thread.locked_at.is_some() || thread.deleted_at.is_some() {
        return HttpResponse::Forbidden().body("This thread is closed.");
    }

    let body = form.body.as_deref().unwrap_or("").trim();
    if body.is_empty() {
        return redirect_with_message(
            &format!("/lecturer/forum/threads/{thread_id}"),
            "Reply cannot be empty.",
        );
    }
    if let Some(parent_id) = form.parent_post_id {
        if !post_belongs_to_thread(db.get_ref(), parent_id, thread_id).await {
            return HttpResponse::BadRequest().body("Parent reply does not belong to this thread.");
        }
    }

    let post_id = match insert_reply(db.get_ref(), thread_id, user.id, form.parent_post_id, body).await {
        Ok(post_id) => post_id,
        Err(error) => return HttpResponse::InternalServerError().body(error.to_string()),
    };

    if let Err(message) =
        save_post_attachments(db.get_ref(), storage.get_ref(), post_id, user.id, form.images).await
    {
        return redirect_with_message(&format!("/lecturer/forum/threads/{thread_id}"), &message);
    }

    if thread.created_by != user.id {
        insert_notification(
            db.get_ref(),
            thread.created_by,
            "Lecturer replied",
            &format!(
                "{} replied to your thread: {}",
                user.display_name, thread.title
            ),
            &format!("/student/forum/threads/{thread_id}"),
        )
        .await;
    }

    redirect(&format!("/lecturer/forum/threads/{thread_id}"))
}

pub async fn edit_student_thread(
    path: web::Path<i32>,
    db: web::Data<PgPool>,
    storage: web::Data<crate::storage::SupabaseStorage>,
    session: Session,
    payload: Multipart,
) -> HttpResponse {
    let user = match crate::auth::require_role(&session, UserRole::Student) {
        Ok(user) => user,
        Err(response) => return response,
    };
    let thread_id = path.into_inner();
    let form = match parse_forum_multipart(payload).await {
        Ok(form) => form,
        Err(message) => {
            return redirect_with_message(&format!("/student/forum/threads/{thread_id}"), &message);
        }
    };
    let Some(thread) = load_thread(db.get_ref(), thread_id, user.id).await else {
        return HttpResponse::NotFound().body("Thread not found.");
    };
    if thread.created_by != user.id || thread.deleted_at.is_some() {
        return HttpResponse::Forbidden().body("You can only edit your own active thread.");
    }

    let title = form.title.as_deref().unwrap_or("").trim();
    let body = form.body.as_deref().unwrap_or("").trim();
    if title.is_empty() || body.is_empty() {
        return redirect_with_message(
            &format!("/student/forum/threads/{thread_id}"),
            "Title and message are required.",
        );
    }

    if let Err(error) = sqlx::query(
        "UPDATE forum_threads
         SET title = $1, body = $2, tags = $3, edited_at = NOW(), updated_at = NOW()
         WHERE id = $4 AND created_by = $5 AND deleted_at IS NULL",
    )
    .bind(title)
    .bind(body)
    .bind(clean_tags(form.tags.as_deref()))
    .bind(thread_id)
    .bind(user.id)
    .execute(db.get_ref())
    .await
    {
        return HttpResponse::InternalServerError().body(error.to_string());
    }

    if let Err(message) = save_thread_attachments(
        db.get_ref(),
        storage.get_ref(),
        thread_id,
        user.id,
        form.images,
    )
    .await
    {
        return redirect_with_message(&format!("/student/forum/threads/{thread_id}"), &message);
    }

    redirect(&format!("/student/forum/threads/{thread_id}"))
}

pub async fn edit_student_post(
    path: web::Path<i32>,
    db: web::Data<PgPool>,
    storage: web::Data<crate::storage::SupabaseStorage>,
    session: Session,
    payload: Multipart,
) -> HttpResponse {
    let form = match parse_forum_multipart(payload).await {
        Ok(form) => form,
        Err(message) => return redirect_with_message("/student/forum", &message),
    };
    edit_post(
        path.into_inner(),
        db.get_ref(),
        storage.get_ref(),
        session,
        form.body.as_deref().unwrap_or("").trim(),
        form.images,
        UserRole::Student,
    )
    .await
}

pub async fn delete_student_thread(
    path: web::Path<i32>,
    db: web::Data<PgPool>,
    session: Session,
    form: web::Form<DeleteForm>,
) -> HttpResponse {
    let user = match crate::auth::require_role(&session, UserRole::Student) {
        Ok(user) => user,
        Err(response) => return response,
    };
    let thread_id = path.into_inner();
    let result = sqlx::query(
        "UPDATE forum_threads
         SET deleted_at = NOW(), deleted_by = $1, delete_reason = $2, updated_at = NOW()
         WHERE id = $3 AND created_by = $1 AND deleted_at IS NULL",
    )
    .bind(user.id)
    .bind(form.reason.as_deref().unwrap_or("Deleted by author"))
    .bind(thread_id)
    .execute(db.get_ref())
    .await;

    match result {
        Ok(done) if done.rows_affected() > 0 => redirect("/student/forum"),
        Ok(_) => HttpResponse::Forbidden().body("You can only delete your own active thread."),
        Err(error) => HttpResponse::InternalServerError().body(error.to_string()),
    }
}

pub async fn delete_student_post(
    path: web::Path<i32>,
    db: web::Data<PgPool>,
    session: Session,
    form: web::Form<DeleteForm>,
) -> HttpResponse {
    let user = match crate::auth::require_role(&session, UserRole::Student) {
        Ok(user) => user,
        Err(response) => return response,
    };
    let post_id = path.into_inner();
    let Some(thread_id) = thread_id_for_post(db.get_ref(), post_id).await else {
        return HttpResponse::NotFound().body("Reply not found.");
    };
    let result = sqlx::query(
        "UPDATE forum_posts
         SET deleted_at = NOW(), deleted_by = $1, delete_reason = $2, updated_at = NOW()
         WHERE id = $3 AND user_id = $1 AND deleted_at IS NULL",
    )
    .bind(user.id)
    .bind(form.reason.as_deref().unwrap_or("Deleted by author"))
    .bind(post_id)
    .execute(db.get_ref())
    .await;

    match result {
        Ok(done) if done.rows_affected() > 0 => {
            redirect(&format!("/student/forum/threads/{thread_id}"))
        }
        Ok(_) => HttpResponse::Forbidden().body("You can only delete your own active reply."),
        Err(error) => HttpResponse::InternalServerError().body(error.to_string()),
    }
}

pub async fn delete_student_attachment(
    path: web::Path<i32>,
    db: web::Data<PgPool>,
    storage: web::Data<crate::storage::SupabaseStorage>,
    session: Session,
    form: web::Form<DeleteForm>,
) -> HttpResponse {
    let user = match crate::auth::require_role(&session, UserRole::Student) {
        Ok(user) => user,
        Err(response) => return response,
    };
    let attachment_id = path.into_inner();
    let Some(ctx) = attachment_context(db.get_ref(), attachment_id).await else {
        return HttpResponse::NotFound().body("Attachment not found.");
    };
    let Some(student_id) = student_id_for_user(db.get_ref(), user.id).await else {
        return HttpResponse::Forbidden().body("Student profile not found.");
    };
    if ctx.uploaded_by != user.id || !student_can_access_course(db.get_ref(), student_id, ctx.course_id).await {
        return HttpResponse::Forbidden().body("You can only remove your own attachment.");
    }

    let _ = storage.delete(&ctx.object_path).await;
    let reason = form.reason.as_deref().unwrap_or("Removed by author");
    if let Err(error) = soft_delete_attachment(db.get_ref(), attachment_id, user.id, reason).await {
        return HttpResponse::InternalServerError().body(error.to_string());
    }

    redirect(&format!("/student/forum/threads/{}", ctx.thread_id))
}

pub async fn edit_lecturer_post(
    path: web::Path<i32>,
    db: web::Data<PgPool>,
    storage: web::Data<crate::storage::SupabaseStorage>,
    session: Session,
    payload: Multipart,
) -> HttpResponse {
    let form = match parse_forum_multipart(payload).await {
        Ok(form) => form,
        Err(message) => return redirect_with_message("/lecturer/forum", &message),
    };
    edit_post(
        path.into_inner(),
        db.get_ref(),
        storage.get_ref(),
        session,
        form.body.as_deref().unwrap_or("").trim(),
        form.images,
        UserRole::Lecturer,
    )
    .await
}

pub async fn moderate_attachment(
    path: web::Path<(i32, String)>,
    db: web::Data<PgPool>,
    storage: web::Data<crate::storage::SupabaseStorage>,
    session: Session,
    form: web::Form<ModerationForm>,
) -> HttpResponse {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };
    let (attachment_id, action) = path.into_inner();
    if action != "delete" {
        return HttpResponse::BadRequest().body("Unknown moderation action.");
    }

    let Some(lecturer_id) = lecturer_id_for_user(db.get_ref(), user.id).await else {
        return HttpResponse::Forbidden().body("Lecturer profile not found.");
    };
    let Some(ctx) = attachment_context(db.get_ref(), attachment_id).await else {
        return HttpResponse::NotFound().body("Attachment not found.");
    };
    if !lecturer_owns_course(db.get_ref(), lecturer_id, ctx.course_id).await {
        return HttpResponse::Forbidden().body("You do not manage this course.");
    }

    let _ = storage.delete(&ctx.object_path).await;
    let reason = form.reason.as_deref().unwrap_or("Removed by lecturer");
    if let Err(error) = soft_delete_attachment(db.get_ref(), attachment_id, user.id, reason).await {
        return HttpResponse::InternalServerError().body(error.to_string());
    }

    log_moderation(
        db.get_ref(),
        user.id,
        "delete",
        "attachment",
        attachment_id,
        Some(ctx.thread_id),
        form.reason.as_deref(),
    )
    .await;

    if ctx.uploaded_by != user.id {
        insert_notification(
            db.get_ref(),
            ctx.uploaded_by,
            "Forum moderation update",
            "A lecturer removed one of your forum images.",
            &format!("/student/forum/threads/{}", ctx.thread_id),
        )
        .await;
    }

    redirect(&format!("/lecturer/forum/threads/{}", ctx.thread_id))
}

pub async fn moderate_thread(
    path: web::Path<(i32, String)>,
    db: web::Data<PgPool>,
    session: Session,
    form: web::Form<ModerationForm>,
) -> HttpResponse {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };
    let (thread_id, action) = path.into_inner();

    let Some(lecturer_id) = lecturer_id_for_user(db.get_ref(), user.id).await else {
        return HttpResponse::Forbidden().body("Lecturer profile not found.");
    };
    let Some(thread) = load_thread(db.get_ref(), thread_id, user.id).await else {
        return HttpResponse::NotFound().body("Thread not found.");
    };
    if !lecturer_owns_course(db.get_ref(), lecturer_id, thread.course_id).await {
        return HttpResponse::Forbidden().body("You do not manage this course.");
    }

    let sql = match action.as_str() {
        "delete" => {
            "UPDATE forum_threads SET deleted_at = NOW(), deleted_by = $1, delete_reason = $2, updated_at = NOW() WHERE id = $3 AND deleted_at IS NULL"
        }
        "pin" => "UPDATE forum_threads SET is_pinned = TRUE, updated_at = NOW() WHERE id = $3",
        "unpin" => "UPDATE forum_threads SET is_pinned = FALSE, updated_at = NOW() WHERE id = $3",
        "answered" => {
            "UPDATE forum_threads SET is_answered = TRUE, updated_at = NOW() WHERE id = $3"
        }
        "unanswered" => {
            "UPDATE forum_threads SET is_answered = FALSE, updated_at = NOW() WHERE id = $3"
        }
        "lock" => {
            "UPDATE forum_threads SET locked_at = NOW(), locked_by = $1, updated_at = NOW() WHERE id = $3 AND locked_at IS NULL"
        }
        "unlock" => {
            "UPDATE forum_threads SET locked_at = NULL, locked_by = NULL, updated_at = NOW() WHERE id = $3"
        }
        _ => return HttpResponse::BadRequest().body("Unknown moderation action."),
    };

    let reason = form.reason.as_deref().unwrap_or("");
    if let Err(error) = sqlx::query(sql)
        .bind(user.id)
        .bind(reason)
        .bind(thread_id)
        .execute(db.get_ref())
        .await
    {
        return HttpResponse::InternalServerError().body(error.to_string());
    }

    log_moderation(
        db.get_ref(),
        user.id,
        &action,
        "thread",
        thread_id,
        Some(thread_id),
        form.reason.as_deref(),
    )
    .await;

    if matches!(action.as_str(), "delete" | "lock" | "unlock") && thread.created_by != user.id {
        insert_notification(
            db.get_ref(),
            thread.created_by,
            "Forum moderation update",
            &format!(
                "A lecturer {} your thread: {}",
                moderation_verb(&action),
                thread.title
            ),
            &format!("/student/forum/threads/{thread_id}"),
        )
        .await;
    }

    redirect(&format!("/lecturer/forum/threads/{thread_id}"))
}

pub async fn moderate_post(
    path: web::Path<(i32, String)>,
    db: web::Data<PgPool>,
    session: Session,
    form: web::Form<ModerationForm>,
) -> HttpResponse {
    let user = match crate::auth::require_role(&session, UserRole::Lecturer) {
        Ok(user) => user,
        Err(response) => return response,
    };
    let (post_id, action) = path.into_inner();
    if action != "delete" {
        return HttpResponse::BadRequest().body("Unknown moderation action.");
    }

    let Some(lecturer_id) = lecturer_id_for_user(db.get_ref(), user.id).await else {
        return HttpResponse::Forbidden().body("Lecturer profile not found.");
    };
    let Some((thread_id, course_id, post_user_id, thread_title)) =
        post_moderation_context(db.get_ref(), post_id).await
    else {
        return HttpResponse::NotFound().body("Reply not found.");
    };
    if !lecturer_owns_course(db.get_ref(), lecturer_id, course_id).await {
        return HttpResponse::Forbidden().body("You do not manage this course.");
    }

    let reason = form.reason.as_deref().unwrap_or("Deleted by lecturer");
    if let Err(error) = sqlx::query(
        "UPDATE forum_posts
         SET deleted_at = NOW(), deleted_by = $1, delete_reason = $2, updated_at = NOW()
         WHERE id = $3 AND deleted_at IS NULL",
    )
    .bind(user.id)
    .bind(reason)
    .bind(post_id)
    .execute(db.get_ref())
    .await
    {
        return HttpResponse::InternalServerError().body(error.to_string());
    }

    log_moderation(
        db.get_ref(),
        user.id,
        "delete",
        "post",
        post_id,
        Some(thread_id),
        form.reason.as_deref(),
    )
    .await;

    if post_user_id != user.id {
        insert_notification(
            db.get_ref(),
            post_user_id,
            "Forum moderation update",
            &format!("A lecturer deleted your reply in: {thread_title}"),
            &format!("/student/forum/threads/{thread_id}"),
        )
        .await;
    }

    redirect(&format!("/lecturer/forum/threads/{thread_id}"))
}

enum ForumAudience<'a> {
    Student {
        user_id: i32,
        display_name: &'a str,
        student_id: i32,
        course_id: Option<i32>,
    },
    Lecturer {
        user_id: i32,
        display_name: &'a str,
        lecturer_id: i32,
        course_id: Option<i32>,
    },
}

impl ForumAudience<'_> {
    fn user_id(&self) -> i32 {
        match self {
            Self::Student { user_id, .. } | Self::Lecturer { user_id, .. } => *user_id,
        }
    }

    fn display_name(&self) -> &str {
        match self {
            Self::Student { display_name, .. } | Self::Lecturer { display_name, .. } => {
                display_name
            }
        }
    }

    fn template(&self) -> &'static str {
        match self {
            Self::Student { .. } => "student/discussionforum.html",
            Self::Lecturer { .. } => "lecturer/forum.html",
        }
    }

    fn detail_template(&self) -> &'static str {
        match self {
            Self::Student { .. } => "student/forum_thread.html",
            Self::Lecturer { .. } => "lecturer/forum_thread.html",
        }
    }

    fn list_path(&self) -> &'static str {
        match self {
            Self::Student { .. } => "/student/forum",
            Self::Lecturer { .. } => "/lecturer/forum",
        }
    }
}

async fn render_forum_list(tmpl: &Tera, db: &PgPool, audience: ForumAudience<'_>) -> HttpResponse {
    let courses = match load_courses(db, &audience).await {
        Ok(courses) => courses,
        Err(error) => return HttpResponse::InternalServerError().body(error.to_string()),
    };
    let threads = match load_threads(db, &audience).await {
        Ok(threads) => threads,
        Err(error) => return HttpResponse::InternalServerError().body(error.to_string()),
    };
    let notifications = load_notifications(db, audience.user_id()).await;

    let mut ctx = Context::new();
    match &audience {
        ForumAudience::Student { display_name, .. } => {
            crate::insert_student_base(&mut ctx, display_name, "Student");
            ctx.insert("student_name", display_name);
        }
        ForumAudience::Lecturer { display_name, .. } => {
            ctx.insert("student_name", display_name);
            ctx.insert("student_id", "");
            ctx.insert("lecturer_name", display_name);
            ctx.insert("is_lecturer", &true);
        }
    }
    ctx.insert("active_page", "forum");
    ctx.insert("courses", &courses);
    ctx.insert("threads", &threads);
    ctx.insert("notifications", &notifications);

    render(tmpl, audience.template(), ctx)
}

async fn render_thread_detail(
    tmpl: &Tera,
    db: &PgPool,
    storage: &crate::storage::SupabaseStorage,
    audience: ForumAudience<'_>,
    thread_id: i32,
) -> HttpResponse {
    let Some(mut thread) = load_thread(db, thread_id, audience.user_id()).await else {
        return HttpResponse::NotFound().body("Thread not found.");
    };

    let allowed = match audience {
        ForumAudience::Student { student_id, .. } => {
            student_can_access_course(db, student_id, thread.course_id).await
        }
        ForumAudience::Lecturer { lecturer_id, .. } => {
            lecturer_owns_course(db, lecturer_id, thread.course_id).await
        }
    };
    if !allowed {
        return HttpResponse::Forbidden().body("You do not have access to this discussion.");
    }

    if thread.deleted_at.is_none() {
        let _ = sqlx::query("UPDATE forum_threads SET view_count = view_count + 1 WHERE id = $1")
            .bind(thread_id)
            .execute(db)
            .await;
    }

    thread.attachments = load_thread_attachments(db, storage, thread_id)
        .await
        .unwrap_or_default();

    let posts = match load_posts(db, storage, thread_id, audience.user_id()).await {
        Ok(posts) => posts,
        Err(error) => return HttpResponse::InternalServerError().body(error.to_string()),
    };
    let actions = match load_moderation_actions(db, thread_id).await {
        Ok(actions) => actions,
        Err(_) => Vec::new(),
    };
    let notifications = load_notifications(db, audience.user_id()).await;

    let mut ctx = Context::new();
    match &audience {
        ForumAudience::Student { display_name, .. } => {
            crate::insert_student_base(&mut ctx, display_name, "Student");
            ctx.insert("student_name", display_name);
        }
        ForumAudience::Lecturer { display_name, .. } => {
            ctx.insert("student_name", display_name);
            ctx.insert("student_id", "");
            ctx.insert("lecturer_name", display_name);
            ctx.insert("is_lecturer", &true);
        }
    }
    ctx.insert("active_page", "forum");
    ctx.insert("thread", &thread);
    ctx.insert("posts", &posts);
    ctx.insert("moderation_actions", &actions);
    ctx.insert("notifications", &notifications);
    ctx.insert("forum_list_path", audience.list_path());

    render(tmpl, audience.detail_template(), ctx)
}

async fn load_courses(
    db: &PgPool,
    audience: &ForumAudience<'_>,
) -> Result<Vec<CourseOption>, sqlx::Error> {
    match audience {
        ForumAudience::Student {
            student_id,
            course_id,
            ..
        } => {
            if let Some(course_id) = course_id {
                sqlx::query_as::<_, CourseOption>(
                    "SELECT c.id, c.course_code AS code, c.course_name AS name
                     FROM courses c
                     JOIN enrollments e ON e.course_id = c.id
                     WHERE e.student_id = $1 AND c.id = $2
                     ORDER BY c.course_code",
                )
                .bind(student_id)
                .bind(course_id)
                .fetch_all(db)
                .await
            } else {
                sqlx::query_as::<_, CourseOption>(
                    "SELECT c.id, c.course_code AS code, c.course_name AS name
                     FROM courses c
                     JOIN enrollments e ON e.course_id = c.id
                     WHERE e.student_id = $1
                     ORDER BY c.course_code",
                )
                .bind(student_id)
                .fetch_all(db)
                .await
            }
        }
        ForumAudience::Lecturer {
            lecturer_id,
            course_id,
            ..
        } => {
            if let Some(course_id) = course_id {
                sqlx::query_as::<_, CourseOption>(
                    "SELECT c.id, c.course_code AS code, c.course_name AS name
                     FROM courses c
                     WHERE c.lecturer_id = $1 AND c.id = $2
                     ORDER BY c.course_code",
                )
                .bind(lecturer_id)
                .bind(course_id)
                .fetch_all(db)
                .await
            } else {
                sqlx::query_as::<_, CourseOption>(
                    "SELECT c.id, c.course_code AS code, c.course_name AS name
                     FROM courses c
                     WHERE c.lecturer_id = $1
                     ORDER BY c.course_code",
                )
                .bind(lecturer_id)
                .fetch_all(db)
                .await
            }
        }
    }
}

async fn load_threads(
    db: &PgPool,
    audience: &ForumAudience<'_>,
) -> Result<Vec<ForumThreadListItem>, sqlx::Error> {
    match audience {
        ForumAudience::Student {
            student_id,
            course_id,
            ..
        } => {
            if let Some(course_id) = course_id {
                sqlx::query_as::<_, ThreadRow>(
                    "SELECT ft.id,
                            ft.title,
                            c.course_code,
                            c.course_name,
                            u.display_name AS author,
                            UPPER(SUBSTRING(u.display_name, 1, 1)) AS author_initials,
                            TO_CHAR(ft.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS created_at,
                            TO_CHAR(COALESCE((SELECT MAX(fp.created_at) FROM forum_posts fp WHERE fp.thread_id = ft.id), ft.updated_at) AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS last_reply_at,
                            (SELECT COUNT(*)::INT FROM forum_posts fp WHERE fp.thread_id = ft.id AND fp.deleted_at IS NULL) AS reply_count,
                            ft.view_count,
                            ft.is_pinned,
                            ft.is_answered,
                            ft.locked_at IS NOT NULL AS is_locked,
                            ft.thread_type = 'announcement' AS is_announcement,
                            ft.created_by = $1 AS is_mine,
                            COALESCE(NULLIF(ft.tags, ''), '') AS tags_text,
                            CASE
                                WHEN ft.deleted_at IS NOT NULL THEN 'This thread has been deleted.'
                                WHEN LENGTH(ft.body) > 180 THEN SUBSTRING(ft.body, 1, 180) || '...'
                                ELSE ft.body
                            END AS preview
                     FROM forum_threads ft
                     JOIN courses c ON c.id = ft.course_id
                     JOIN users u ON u.id = ft.created_by
                     JOIN enrollments e ON e.course_id = c.id
                     WHERE e.student_id = $2 AND c.id = $3
                     ORDER BY ft.is_pinned DESC, ft.updated_at DESC, ft.created_at DESC",
                )
                .bind(audience.user_id())
                .bind(student_id)
                .bind(course_id)
                .fetch_all(db)
                .await
                .map(map_thread_rows)
            } else {
                sqlx::query_as::<_, ThreadRow>(
                    "SELECT ft.id,
                            ft.title,
                            c.course_code,
                            c.course_name,
                            u.display_name AS author,
                            UPPER(SUBSTRING(u.display_name, 1, 1)) AS author_initials,
                            TO_CHAR(ft.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS created_at,
                            TO_CHAR(COALESCE((SELECT MAX(fp.created_at) FROM forum_posts fp WHERE fp.thread_id = ft.id), ft.updated_at) AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS last_reply_at,
                            (SELECT COUNT(*)::INT FROM forum_posts fp WHERE fp.thread_id = ft.id AND fp.deleted_at IS NULL) AS reply_count,
                            ft.view_count,
                            ft.is_pinned,
                            ft.is_answered,
                            ft.locked_at IS NOT NULL AS is_locked,
                            ft.thread_type = 'announcement' AS is_announcement,
                            ft.created_by = $1 AS is_mine,
                            COALESCE(NULLIF(ft.tags, ''), '') AS tags_text,
                            CASE
                                WHEN ft.deleted_at IS NOT NULL THEN 'This thread has been deleted.'
                                WHEN LENGTH(ft.body) > 180 THEN SUBSTRING(ft.body, 1, 180) || '...'
                                ELSE ft.body
                            END AS preview
                     FROM forum_threads ft
                     JOIN courses c ON c.id = ft.course_id
                     JOIN users u ON u.id = ft.created_by
                     JOIN enrollments e ON e.course_id = c.id
                     WHERE e.student_id = $2
                     ORDER BY ft.is_pinned DESC, ft.updated_at DESC, ft.created_at DESC",
                )
                .bind(audience.user_id())
                .bind(student_id)
                .fetch_all(db)
                .await
                .map(map_thread_rows)
            }
        }
        ForumAudience::Lecturer {
            lecturer_id,
            course_id,
            ..
        } => {
            if let Some(course_id) = course_id {
                sqlx::query_as::<_, ThreadRow>(
                    "SELECT ft.id,
                            ft.title,
                            c.course_code,
                            c.course_name,
                            u.display_name AS author,
                            UPPER(SUBSTRING(u.display_name, 1, 1)) AS author_initials,
                            TO_CHAR(ft.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS created_at,
                            TO_CHAR(COALESCE((SELECT MAX(fp.created_at) FROM forum_posts fp WHERE fp.thread_id = ft.id), ft.updated_at) AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS last_reply_at,
                            (SELECT COUNT(*)::INT FROM forum_posts fp WHERE fp.thread_id = ft.id AND fp.deleted_at IS NULL) AS reply_count,
                            ft.view_count,
                            ft.is_pinned,
                            ft.is_answered,
                            ft.locked_at IS NOT NULL AS is_locked,
                            ft.thread_type = 'announcement' AS is_announcement,
                            ft.created_by = $1 AS is_mine,
                            COALESCE(NULLIF(ft.tags, ''), '') AS tags_text,
                            CASE
                                WHEN ft.deleted_at IS NOT NULL THEN 'This thread has been deleted.'
                                WHEN LENGTH(ft.body) > 180 THEN SUBSTRING(ft.body, 1, 180) || '...'
                                ELSE ft.body
                            END AS preview
                     FROM forum_threads ft
                     JOIN courses c ON c.id = ft.course_id
                     JOIN users u ON u.id = ft.created_by
                     WHERE c.lecturer_id = $2 AND c.id = $3
                     ORDER BY ft.is_pinned DESC, ft.updated_at DESC, ft.created_at DESC",
                )
                .bind(audience.user_id())
                .bind(lecturer_id)
                .bind(course_id)
                .fetch_all(db)
                .await
                .map(map_thread_rows)
            } else {
                sqlx::query_as::<_, ThreadRow>(
                    "SELECT ft.id,
                            ft.title,
                            c.course_code,
                            c.course_name,
                            u.display_name AS author,
                            UPPER(SUBSTRING(u.display_name, 1, 1)) AS author_initials,
                            TO_CHAR(ft.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS created_at,
                            TO_CHAR(COALESCE((SELECT MAX(fp.created_at) FROM forum_posts fp WHERE fp.thread_id = ft.id), ft.updated_at) AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS last_reply_at,
                            (SELECT COUNT(*)::INT FROM forum_posts fp WHERE fp.thread_id = ft.id AND fp.deleted_at IS NULL) AS reply_count,
                            ft.view_count,
                            ft.is_pinned,
                            ft.is_answered,
                            ft.locked_at IS NOT NULL AS is_locked,
                            ft.thread_type = 'announcement' AS is_announcement,
                            ft.created_by = $1 AS is_mine,
                            COALESCE(NULLIF(ft.tags, ''), '') AS tags_text,
                            CASE
                                WHEN ft.deleted_at IS NOT NULL THEN 'This thread has been deleted.'
                                WHEN LENGTH(ft.body) > 180 THEN SUBSTRING(ft.body, 1, 180) || '...'
                                ELSE ft.body
                            END AS preview
                     FROM forum_threads ft
                     JOIN courses c ON c.id = ft.course_id
                     JOIN users u ON u.id = ft.created_by
                     WHERE c.lecturer_id = $2
                     ORDER BY ft.is_pinned DESC, ft.updated_at DESC, ft.created_at DESC",
                )
                .bind(audience.user_id())
                .bind(lecturer_id)
                .fetch_all(db)
                .await
                .map(map_thread_rows)
            }
        }
    }
}

#[derive(FromRow)]
struct ThreadRow {
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
    is_locked: bool,
    is_announcement: bool,
    is_mine: bool,
    tags_text: String,
    preview: String,
}

fn map_thread_rows(rows: Vec<ThreadRow>) -> Vec<ForumThreadListItem> {
    rows.into_iter()
        .map(|row| ForumThreadListItem {
            id: row.id,
            title: row.title,
            course_code: row.course_code,
            course_name: row.course_name,
            author: row.author,
            author_initials: row.author_initials,
            created_at: row.created_at,
            last_reply_at: row.last_reply_at,
            reply_count: row.reply_count,
            view_count: row.view_count,
            is_pinned: row.is_pinned,
            is_answered: row.is_answered,
            is_locked: row.is_locked,
            is_announcement: row.is_announcement,
            is_mine: row.is_mine,
            tags: parse_tags(Some(&row.tags_text)),
            preview: row.preview,
        })
        .collect()
}

async fn load_thread(db: &PgPool, thread_id: i32, user_id: i32) -> Option<ForumThreadDetail> {
    let row = sqlx::query_as::<_, ThreadDetailRow>(
        "SELECT ft.id,
                ft.course_id,
                c.course_code,
                c.course_name,
                ft.created_by,
                ft.title,
                ft.body,
                COALESCE(NULLIF(ft.tags, ''), '') AS tags_text,
                u.display_name AS author,
                UPPER(SUBSTRING(u.display_name, 1, 1)) AS author_initials,
                TO_CHAR(ft.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS created_at,
                TO_CHAR(ft.updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS updated_at,
                CASE WHEN ft.edited_at IS NULL THEN NULL ELSE TO_CHAR(ft.edited_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') END AS edited_at,
                CASE WHEN ft.deleted_at IS NULL THEN NULL ELSE TO_CHAR(ft.deleted_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') END AS deleted_at,
                du.display_name AS deleted_by_name,
                ft.delete_reason,
                CASE WHEN ft.locked_at IS NULL THEN NULL ELSE TO_CHAR(ft.locked_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') END AS locked_at,
                lu.display_name AS locked_by_name,
                ft.is_pinned,
                ft.is_answered,
                ft.thread_type = 'announcement' AS is_announcement,
                ft.view_count,
                ft.created_by = $2 AS is_mine
         FROM forum_threads ft
         JOIN courses c ON c.id = ft.course_id
         JOIN users u ON u.id = ft.created_by
         LEFT JOIN users du ON du.id = ft.deleted_by
         LEFT JOIN users lu ON lu.id = ft.locked_by
         WHERE ft.id = $1",
    )
    .bind(thread_id)
    .bind(user_id)
    .fetch_optional(db)
    .await
    .ok()
    .flatten()?;

    Some(ForumThreadDetail {
        id: row.id,
        course_id: row.course_id,
        course_code: row.course_code,
        course_name: row.course_name,
        created_by: row.created_by,
        title: row.title,
        body: row.body,
        tags: parse_tags(Some(&row.tags_text)),
        author: row.author,
        author_initials: row.author_initials,
        created_at: row.created_at,
        updated_at: row.updated_at,
        edited_at: row.edited_at,
        deleted_at: row.deleted_at,
        deleted_by_name: row.deleted_by_name,
        delete_reason: row.delete_reason,
        locked_at: row.locked_at,
        locked_by_name: row.locked_by_name,
        is_pinned: row.is_pinned,
        is_answered: row.is_answered,
        is_announcement: row.is_announcement,
        view_count: row.view_count,
        is_mine: row.is_mine,
        attachments: Vec::new(),
    })
}

#[derive(FromRow)]
struct ThreadDetailRow {
    id: i32,
    course_id: i32,
    course_code: String,
    course_name: String,
    created_by: i32,
    title: String,
    body: String,
    tags_text: String,
    author: String,
    author_initials: String,
    created_at: String,
    updated_at: String,
    edited_at: Option<String>,
    deleted_at: Option<String>,
    deleted_by_name: Option<String>,
    delete_reason: Option<String>,
    locked_at: Option<String>,
    locked_by_name: Option<String>,
    is_pinned: bool,
    is_answered: bool,
    is_announcement: bool,
    view_count: i32,
    is_mine: bool,
}

async fn load_posts(
    db: &PgPool,
    storage: &crate::storage::SupabaseStorage,
    thread_id: i32,
    current_user_id: i32,
) -> Result<Vec<ForumPostView>, sqlx::Error> {
    let rows = sqlx::query_as::<_, ForumPostFlat>(
        "SELECT fp.id,
                fp.thread_id,
                fp.user_id,
                fp.parent_post_id,
                fp.body,
                u.display_name AS author,
                UPPER(SUBSTRING(u.display_name, 1, 1)) AS author_initials,
                TO_CHAR(fp.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS created_at,
                TO_CHAR(fp.updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS updated_at,
                CASE WHEN fp.edited_at IS NULL THEN NULL ELSE TO_CHAR(fp.edited_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') END AS edited_at,
                CASE WHEN fp.deleted_at IS NULL THEN NULL ELSE TO_CHAR(fp.deleted_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') END AS deleted_at,
                du.display_name AS deleted_by_name,
                fp.delete_reason,
                fp.user_id = $2 AS is_mine
         FROM forum_posts fp
         JOIN users u ON u.id = fp.user_id
         LEFT JOIN users du ON du.id = fp.deleted_by
         WHERE fp.thread_id = $1
         ORDER BY fp.created_at ASC",
    )
    .bind(thread_id)
    .bind(current_user_id)
    .fetch_all(db)
    .await?;

    let mut flattened = Vec::new();
    flatten_posts(None, 0, &rows, &mut flattened);
    for post in &mut flattened {
        post.attachments = load_post_attachments(db, storage, post.id)
            .await
            .unwrap_or_default();
    }
    Ok(flattened)
}

fn flatten_posts(
    parent_id: Option<i32>,
    depth: i32,
    rows: &[ForumPostFlat],
    output: &mut Vec<ForumPostView>,
) {
    for row in rows.iter().filter(|post| post.parent_post_id == parent_id) {
        output.push(ForumPostView {
            id: row.id,
            thread_id: row.thread_id,
            user_id: row.user_id,
            parent_post_id: row.parent_post_id,
            body: row.body.clone(),
            author: row.author.clone(),
            author_initials: row.author_initials.clone(),
            created_at: row.created_at.clone(),
            updated_at: row.updated_at.clone(),
            edited_at: row.edited_at.clone(),
            deleted_at: row.deleted_at.clone(),
            deleted_by_name: row.deleted_by_name.clone(),
            delete_reason: row.delete_reason.clone(),
            is_mine: row.is_mine,
            depth: depth.min(4),
            attachments: Vec::new(),
        });
        flatten_posts(Some(row.id), depth + 1, rows, output);
    }
}

async fn load_moderation_actions(
    db: &PgPool,
    thread_id: i32,
) -> Result<Vec<ModerationActionView>, sqlx::Error> {
    sqlx::query_as::<_, ModerationActionView>(
        "SELECT fma.action,
                fma.target_type,
                NULLIF(fma.reason, '') AS reason,
                u.display_name AS moderator,
                TO_CHAR(fma.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS created_at
         FROM forum_moderation_actions fma
         JOIN users u ON u.id = fma.moderator_user_id
         WHERE fma.thread_id = $1
         ORDER BY fma.created_at DESC
         LIMIT 20",
    )
    .bind(thread_id)
    .fetch_all(db)
    .await
}

async fn load_thread_attachments(
    db: &PgPool,
    storage: &crate::storage::SupabaseStorage,
    thread_id: i32,
) -> Result<Vec<ForumAttachmentView>, sqlx::Error> {
    load_attachments(db, storage, Some(thread_id), None).await
}

async fn load_post_attachments(
    db: &PgPool,
    storage: &crate::storage::SupabaseStorage,
    post_id: i32,
) -> Result<Vec<ForumAttachmentView>, sqlx::Error> {
    load_attachments(db, storage, None, Some(post_id)).await
}

async fn load_attachments(
    db: &PgPool,
    storage: &crate::storage::SupabaseStorage,
    thread_id: Option<i32>,
    post_id: Option<i32>,
) -> Result<Vec<ForumAttachmentView>, sqlx::Error> {
    let rows = if let Some(thread_id) = thread_id {
        sqlx::query_as::<_, AttachmentRow>(
            "SELECT id, original_filename, content_type, file_size, object_path
             FROM forum_attachments
             WHERE thread_id = $1 AND deleted_at IS NULL
             ORDER BY created_at ASC",
        )
        .bind(thread_id)
        .fetch_all(db)
        .await?
    } else if let Some(post_id) = post_id {
        sqlx::query_as::<_, AttachmentRow>(
            "SELECT id, original_filename, content_type, file_size, object_path
             FROM forum_attachments
             WHERE post_id = $1 AND deleted_at IS NULL
             ORDER BY created_at ASC",
        )
        .bind(post_id)
        .fetch_all(db)
        .await?
    } else {
        Vec::new()
    };

    Ok(rows
        .into_iter()
        .map(|row| ForumAttachmentView {
            id: row.id,
            original_filename: row.original_filename,
            content_type: row.content_type,
            file_size: row.file_size,
            public_url: storage.public_url(&row.object_path),
        })
        .collect())
}

#[derive(FromRow)]
struct AttachmentRow {
    id: i32,
    original_filename: String,
    content_type: String,
    file_size: i32,
    object_path: String,
}

async fn save_thread_attachments(
    db: &PgPool,
    storage: &crate::storage::SupabaseStorage,
    thread_id: i32,
    user_id: i32,
    images: Vec<PendingUpload>,
) -> Result<(), String> {
    if images.is_empty() {
        return Ok(());
    }
    let current_count = active_attachment_count(db, Some(thread_id), None).await?;
    if current_count + images.len() > MAX_ATTACHMENTS_PER_ITEM {
        return Err(format!(
            "A thread can have at most {MAX_ATTACHMENTS_PER_ITEM} images."
        ));
    }

    for image in images {
        let object_path = build_attachment_path("threads", thread_id, user_id, &image.content_type);
        save_attachment(
            db,
            storage,
            Some(thread_id),
            None,
            user_id,
            object_path,
            image,
        )
        .await?;
    }
    Ok(())
}

async fn save_post_attachments(
    db: &PgPool,
    storage: &crate::storage::SupabaseStorage,
    post_id: i32,
    user_id: i32,
    images: Vec<PendingUpload>,
) -> Result<(), String> {
    if images.is_empty() {
        return Ok(());
    }
    let current_count = active_attachment_count(db, None, Some(post_id)).await?;
    if current_count + images.len() > MAX_ATTACHMENTS_PER_ITEM {
        return Err(format!(
            "A reply can have at most {MAX_ATTACHMENTS_PER_ITEM} images."
        ));
    }

    for image in images {
        let object_path = build_attachment_path("posts", post_id, user_id, &image.content_type);
        save_attachment(
            db,
            storage,
            None,
            Some(post_id),
            user_id,
            object_path,
            image,
        )
        .await?;
    }
    Ok(())
}

async fn active_attachment_count(
    db: &PgPool,
    thread_id: Option<i32>,
    post_id: Option<i32>,
) -> Result<usize, String> {
    let count = if let Some(thread_id) = thread_id {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM forum_attachments
             WHERE thread_id = $1 AND deleted_at IS NULL",
        )
        .bind(thread_id)
        .fetch_one(db)
        .await
    } else if let Some(post_id) = post_id {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM forum_attachments
             WHERE post_id = $1 AND deleted_at IS NULL",
        )
        .bind(post_id)
        .fetch_one(db)
        .await
    } else {
        Ok(0)
    }
    .map_err(|error| error.to_string())?;

    Ok(count as usize)
}

async fn save_attachment(
    db: &PgPool,
    storage: &crate::storage::SupabaseStorage,
    thread_id: Option<i32>,
    post_id: Option<i32>,
    user_id: i32,
    object_path: String,
    image: PendingUpload,
) -> Result<(), String> {
    if storage.base_url.is_empty()
        || storage.bucket.is_empty()
        || storage.service_role_key.is_empty()
    {
        return Err("Supabase Storage is not configured in .env.".to_string());
    }

    storage
        .upload(&object_path, image.bytes.clone(), &image.content_type)
        .await?;

    let insert_result = sqlx::query(
        "INSERT INTO forum_attachments
            (thread_id, post_id, uploaded_by, object_path, original_filename, content_type, file_size)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(thread_id)
    .bind(post_id)
    .bind(user_id)
    .bind(&object_path)
    .bind(&image.filename)
    .bind(&image.content_type)
    .bind(image.bytes.len() as i32)
    .execute(db)
    .await;

    if let Err(error) = insert_result {
        let _ = storage.delete(&object_path).await;
        return Err(error.to_string());
    }

    Ok(())
}

async fn insert_thread(
    db: &PgPool,
    course_id: i32,
    user_id: i32,
    title: &str,
    body: &str,
    tags: Option<&str>,
    thread_type: &str,
) -> Result<i32, sqlx::Error> {
    sqlx::query_scalar::<_, i32>(
        "INSERT INTO forum_threads (course_id, created_by, title, body, tags, thread_type)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id",
    )
    .bind(course_id)
    .bind(user_id)
    .bind(title)
    .bind(body)
    .bind(clean_tags(tags))
    .bind(thread_type)
    .fetch_one(db)
    .await
}

async fn insert_reply(
    db: &PgPool,
    thread_id: i32,
    user_id: i32,
    parent_post_id: Option<i32>,
    body: &str,
) -> Result<i32, sqlx::Error> {
    let mut tx = db.begin().await?;
    let post_id = sqlx::query_scalar::<_, i32>(
        "INSERT INTO forum_posts (thread_id, user_id, parent_post_id, body)
         VALUES ($1, $2, $3, $4)
         RETURNING id",
    )
    .bind(thread_id)
    .bind(user_id)
    .bind(parent_post_id)
    .bind(body)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE forum_threads
         SET reply_count = reply_count + 1, updated_at = NOW()
         WHERE id = $1",
    )
    .bind(thread_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(post_id)
}

async fn edit_post(
    post_id: i32,
    db: &PgPool,
    storage: &crate::storage::SupabaseStorage,
    session: Session,
    body: &str,
    images: Vec<PendingUpload>,
    required_role: UserRole,
) -> HttpResponse {
    let user = match crate::auth::require_role(&session, required_role) {
        Ok(user) => user,
        Err(response) => return response,
    };
    if body.is_empty() {
        return redirect_with_message("/student/forum", "Reply cannot be empty.");
    }

    let Some(thread_id) = thread_id_for_post(db, post_id).await else {
        return HttpResponse::NotFound().body("Reply not found.");
    };
    let result = sqlx::query(
        "UPDATE forum_posts
         SET body = $1, edited_at = NOW(), updated_at = NOW()
         WHERE id = $2 AND user_id = $3 AND deleted_at IS NULL",
    )
    .bind(body)
    .bind(post_id)
    .bind(user.id)
    .execute(db)
    .await;

    let target = match required_role {
        UserRole::Student => format!("/student/forum/threads/{thread_id}"),
        UserRole::Lecturer => format!("/lecturer/forum/threads/{thread_id}"),
        UserRole::Admin => "/".to_string(),
    };

    match result {
        Ok(done) if done.rows_affected() > 0 => {
            if let Err(message) = save_post_attachments(db, storage, post_id, user.id, images).await
            {
                return redirect_with_message(&target, &message);
            }
            redirect(&target)
        }
        Ok(_) => HttpResponse::Forbidden().body("You can only edit your own active reply."),
        Err(error) => HttpResponse::InternalServerError().body(error.to_string()),
    }
}

async fn student_id_for_user(db: &PgPool, user_id: i32) -> Option<i32> {
    sqlx::query_scalar::<_, i32>("SELECT id FROM students WHERE user_id = $1")
        .bind(user_id)
        .fetch_optional(db)
        .await
        .ok()
        .flatten()
}

async fn lecturer_id_for_user(db: &PgPool, user_id: i32) -> Option<i32> {
    sqlx::query_scalar::<_, i32>("SELECT id FROM lecturers WHERE user_id = $1")
        .bind(user_id)
        .fetch_optional(db)
        .await
        .ok()
        .flatten()
}

async fn student_can_access_course(db: &PgPool, student_id: i32, course_id: i32) -> bool {
    sqlx::query_scalar::<_, i32>(
        "SELECT 1 FROM enrollments WHERE student_id = $1 AND course_id = $2",
    )
    .bind(student_id)
    .bind(course_id)
    .fetch_optional(db)
    .await
    .ok()
    .flatten()
    .is_some()
}

async fn lecturer_owns_course(db: &PgPool, lecturer_id: i32, course_id: i32) -> bool {
    sqlx::query_scalar::<_, i32>("SELECT 1 FROM courses WHERE lecturer_id = $1 AND id = $2")
        .bind(lecturer_id)
        .bind(course_id)
        .fetch_optional(db)
        .await
        .ok()
        .flatten()
        .is_some()
}

async fn post_belongs_to_thread(db: &PgPool, post_id: i32, thread_id: i32) -> bool {
    sqlx::query_scalar::<_, i32>("SELECT 1 FROM forum_posts WHERE id = $1 AND thread_id = $2")
        .bind(post_id)
        .bind(thread_id)
        .fetch_optional(db)
        .await
        .ok()
        .flatten()
        .is_some()
}

async fn thread_id_for_post(db: &PgPool, post_id: i32) -> Option<i32> {
    sqlx::query_scalar::<_, i32>("SELECT thread_id FROM forum_posts WHERE id = $1")
        .bind(post_id)
        .fetch_optional(db)
        .await
        .ok()
        .flatten()
}

async fn post_moderation_context(db: &PgPool, post_id: i32) -> Option<(i32, i32, i32, String)> {
    #[derive(FromRow)]
    struct Row {
        thread_id: i32,
        course_id: i32,
        user_id: i32,
        title: String,
    }

    let row = sqlx::query_as::<_, Row>(
        "SELECT fp.thread_id, ft.course_id, fp.user_id, ft.title
         FROM forum_posts fp
         JOIN forum_threads ft ON ft.id = fp.thread_id
         WHERE fp.id = $1",
    )
    .bind(post_id)
    .fetch_optional(db)
    .await
    .ok()
    .flatten()?;
    Some((row.thread_id, row.course_id, row.user_id, row.title))
}

#[derive(FromRow)]
struct AttachmentContext {
    thread_id: i32,
    course_id: i32,
    uploaded_by: i32,
    object_path: String,
}

async fn attachment_context(db: &PgPool, attachment_id: i32) -> Option<AttachmentContext> {
    sqlx::query_as::<_, AttachmentContext>(
        "SELECT COALESCE(fa.thread_id, fp.thread_id) AS thread_id,
                ft.course_id,
                fa.uploaded_by,
                fa.object_path
         FROM forum_attachments fa
         LEFT JOIN forum_posts fp ON fp.id = fa.post_id
         JOIN forum_threads ft ON ft.id = COALESCE(fa.thread_id, fp.thread_id)
         WHERE fa.id = $1 AND fa.deleted_at IS NULL",
    )
    .bind(attachment_id)
    .fetch_optional(db)
    .await
    .ok()
    .flatten()
}

async fn soft_delete_attachment(
    db: &PgPool,
    attachment_id: i32,
    user_id: i32,
    reason: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE forum_attachments
         SET deleted_at = NOW(), deleted_by = $1, delete_reason = $2
         WHERE id = $3 AND deleted_at IS NULL",
    )
    .bind(user_id)
    .bind(reason)
    .bind(attachment_id)
    .execute(db)
    .await?;
    Ok(())
}

async fn notify_course_lecturer(
    db: &PgPool,
    course_id: i32,
    title: &str,
    message: &str,
    link_url: &str,
) {
    let user_id = sqlx::query_scalar::<_, i32>(
        "SELECT l.user_id
         FROM courses c
         JOIN lecturers l ON l.id = c.lecturer_id
         WHERE c.id = $1",
    )
    .bind(course_id)
    .fetch_optional(db)
    .await
    .ok()
    .flatten();

    if let Some(user_id) = user_id {
        insert_notification(db, user_id, title, message, link_url).await;
    }
}

async fn notify_enrolled_students(
    db: &PgPool,
    course_id: i32,
    title: &str,
    message: &str,
    link_url: &str,
) {
    let user_ids = sqlx::query_scalar::<_, i32>(
        "SELECT s.user_id
         FROM enrollments e
         JOIN students s ON s.id = e.student_id
         WHERE e.course_id = $1",
    )
    .bind(course_id)
    .fetch_all(db)
    .await
    .unwrap_or_default();

    for user_id in user_ids {
        insert_notification(db, user_id, title, message, link_url).await;
    }
}

async fn insert_notification(
    db: &PgPool,
    user_id: i32,
    title: &str,
    message: &str,
    link_url: &str,
) {
    let _ = sqlx::query(
        "INSERT INTO notifications (user_id, title, message, link_url)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(user_id)
    .bind(title)
    .bind(message)
    .bind(link_url)
    .execute(db)
    .await;
}

async fn load_notifications(db: &PgPool, user_id: i32) -> Vec<crate::NotificationContext> {
    #[derive(FromRow)]
    struct Row {
        title: String,
        message: String,
        created_at: String,
    }

    sqlx::query_as::<_, Row>(
        "SELECT title,
                message,
                TO_CHAR(created_at, 'DD Mon HH24:MI') AS created_at
         FROM notifications
         WHERE user_id = $1 AND is_read = FALSE
         ORDER BY created_at DESC
         LIMIT 5",
    )
    .bind(user_id)
    .fetch_all(db)
    .await
    .unwrap_or_default()
    .into_iter()
    .map(|row| crate::NotificationContext {
        icon_class: "bi bi-chat-left-text".to_string(),
        text: row.title,
        sub_text: format!("{} · {}", row.message, row.created_at),
    })
    .collect()
}

async fn log_moderation(
    db: &PgPool,
    moderator_user_id: i32,
    action: &str,
    target_type: &str,
    target_id: i32,
    thread_id: Option<i32>,
    reason: Option<&str>,
) {
    let _ = sqlx::query(
        "INSERT INTO forum_moderation_actions
            (moderator_user_id, action, target_type, target_id, thread_id, reason)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(moderator_user_id)
    .bind(action)
    .bind(target_type)
    .bind(target_id)
    .bind(thread_id)
    .bind(reason.unwrap_or(""))
    .execute(db)
    .await;
}

async fn parse_forum_multipart(mut payload: Multipart) -> Result<ForumMultipartData, String> {
    let mut data = ForumMultipartData::default();

    while let Some(mut field) = payload.try_next().await.map_err(|error| error.to_string())? {
        let Some(disposition) = field.content_disposition() else {
            continue;
        };
        let name = disposition.get_name().unwrap_or("").to_string();
        let filename = disposition.get_filename().map(|value| value.to_string());
        let content_type = field
            .content_type()
            .map(|mime| mime.to_string())
            .unwrap_or_else(|| "application/octet-stream".to_string());

        let mut bytes = Vec::new();
        while let Some(chunk) = field.try_next().await.map_err(|error| error.to_string())? {
            if filename.is_some() && bytes.len() + chunk.len() > MAX_ATTACHMENT_BYTES {
                return Err(format!(
                    "Each image must be {MAX_ATTACHMENT_BYTES} bytes or smaller."
                ));
            }
            bytes.extend_from_slice(&chunk);
        }

        if let Some(filename) = filename {
            if filename.trim().is_empty() || bytes.is_empty() {
                continue;
            }
            if name != "images" {
                continue;
            }
            if data.images.len() >= MAX_ATTACHMENTS_PER_ITEM {
                return Err(format!(
                    "You can upload at most {MAX_ATTACHMENTS_PER_ITEM} images."
                ));
            }
            if !matches!(content_type.as_str(), "image/jpeg" | "image/png") {
                return Err("Only JPG and PNG images are allowed.".to_string());
            }
            data.images.push(PendingUpload {
                filename: clean_filename(&filename),
                content_type,
                bytes,
            });
            continue;
        }

        let value = String::from_utf8(bytes)
            .map_err(|_| "Form text must be valid UTF-8.".to_string())?
            .trim()
            .to_string();

        match name.as_str() {
            "course_id" => data.course_id = value.parse::<i32>().ok(),
            "title" => data.title = Some(value),
            "body" => data.body = Some(value),
            "tags" => data.tags = Some(value),
            "thread_type" => data.thread_type = Some(value),
            "parent_post_id" => data.parent_post_id = value.parse::<i32>().ok(),
            _ => {}
        }
    }

    Ok(data)
}

fn build_attachment_path(kind: &str, owner_id: i32, user_id: i32, content_type: &str) -> String {
    let extension = if content_type == "image/png" {
        "png"
    } else {
        "jpg"
    };
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("forum/{kind}/{owner_id}/{user_id}_{nonce}.{extension}")
}

fn clean_filename(filename: &str) -> String {
    let cleaned: String = filename
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    cleaned.trim_matches('_').chars().take(120).collect()
}

fn parse_tags(tags: Option<&str>) -> Vec<String> {
    tags.unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn clean_tags(tags: Option<&str>) -> String {
    parse_tags(tags).join(", ")
}

fn moderation_verb(action: &str) -> &'static str {
    match action {
        "delete" => "deleted",
        "lock" => "locked",
        "unlock" => "unlocked",
        _ => "updated",
    }
}

fn render(tmpl: &Tera, template: &str, ctx: Context) -> HttpResponse {
    match tmpl.render(template, &ctx) {
        Ok(rendered) => HttpResponse::Ok().content_type("text/html").body(rendered),
        Err(error) => HttpResponse::InternalServerError().body(error.to_string()),
    }
}

fn redirect(location: &str) -> HttpResponse {
    HttpResponse::SeeOther()
        .insert_header((header::LOCATION, location))
        .finish()
}

fn redirect_with_message(location: &str, message: &str) -> HttpResponse {
    let encoded = percent_encode(message);
    let separator = if location.contains('?') { '&' } else { '?' };
    redirect(&format!("{location}{separator}message={encoded}"))
}

fn percent_encode(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            b' ' => vec!['+'],
            _ => {
                let hex = format!("%{byte:02X}");
                hex.chars().collect()
            }
        })
        .collect()
}
