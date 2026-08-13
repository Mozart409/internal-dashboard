//! The single data-access contract. The UI, the REST API and the MCP server
//! all go through these functions; none of them write SQL themselves.

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppError;
use crate::models::{Link, NewLink, UpdateLink};

/// All links, newest first. `tag` filters to links carrying that tag.
pub async fn list_links(pool: &PgPool, tag: Option<&str>) -> Result<Vec<Link>, AppError> {
    let links = sqlx::query_as!(
        Link,
        r#"
        select id, url, title, description, tags, created_at, updated_at
        from links
        where $1::text is null or $1 = any(tags)
        order by created_at desc
        "#,
        tag
    )
    .fetch_all(pool)
    .await?;

    Ok(links)
}

pub async fn get_link(pool: &PgPool, id: Uuid) -> Result<Option<Link>, AppError> {
    let link = sqlx::query_as!(
        Link,
        r#"
        select id, url, title, description, tags, created_at, updated_at
        from links
        where id = $1
        "#,
        id
    )
    .fetch_optional(pool)
    .await?;

    Ok(link)
}

/// Case-insensitive substring match across title, url and description.
pub async fn search_links(pool: &PgPool, q: &str, limit: i64) -> Result<Vec<Link>, AppError> {
    let pattern = format!("%{}%", q.trim());

    let links = sqlx::query_as!(
        Link,
        r#"
        select id, url, title, description, tags, created_at, updated_at
        from links
        where title ilike $1 or url ilike $1 or coalesce(description, '') ilike $1
        order by created_at desc
        limit $2
        "#,
        pattern,
        limit.clamp(1, 500)
    )
    .fetch_all(pool)
    .await?;

    Ok(links)
}

pub async fn create_link(pool: &PgPool, new: &NewLink) -> Result<Link, AppError> {
    let link = sqlx::query_as!(
        Link,
        r#"
        insert into links (url, title, description, tags)
        values ($1, $2, $3, $4)
        returning id, url, title, description, tags, created_at, updated_at
        "#,
        new.url,
        new.title,
        new.description,
        &new.tags
    )
    .fetch_one(pool)
    .await?;

    Ok(link)
}

/// Partial update: `None` fields keep their current value. `description` is
/// deliberately collapsed with the existing value too, so omitting it does not
/// clear it.
pub async fn update_link(
    pool: &PgPool,
    id: Uuid,
    up: &UpdateLink,
) -> Result<Option<Link>, AppError> {
    let link = sqlx::query_as!(
        Link,
        r#"
        update links
        set url         = coalesce($2, url),
            title       = coalesce($3, title),
            description = coalesce($4, description),
            tags        = coalesce($5, tags),
            updated_at  = now()
        where id = $1
        returning id, url, title, description, tags, created_at, updated_at
        "#,
        id,
        up.url.as_deref(),
        up.title.as_deref(),
        up.description.as_deref(),
        up.tags.as_deref()
    )
    .fetch_optional(pool)
    .await?;

    Ok(link)
}

/// Returns whether a row was actually removed.
pub async fn delete_link(pool: &PgPool, id: Uuid) -> Result<bool, AppError> {
    let result = sqlx::query!("delete from links where id = $1", id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}
