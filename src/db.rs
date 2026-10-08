//! The single data-access contract. The UI, the REST API and the MCP server
//! all go through these functions; none of them write SQL themselves.

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppError;
use crate::models::{Link, LinkSort, NewLink, UpdateLink, validate_url};

/// All links in `sort` order. `tag` filters to links carrying that tag.
///
/// # Errors
/// Returns [`AppError::Database`] if the query fails.
pub async fn list_links(
    pool: &PgPool,
    tag: Option<&str>,
    sort: LinkSort,
) -> Result<Vec<Link>, AppError> {
    let links = sqlx::query_as!(
        Link,
        r#"
        select id, url, title, description, tags, clicks, created_at, updated_at
        from links
        where $1::text is null or $1 = any(tags)
        order by
            case when $2 = 'title' then lower(title) end asc,
            case when $2 = 'clicks' then clicks end desc,
            created_at desc,
            id
        "#,
        tag,
        sort.as_str()
    )
    .fetch_all(pool)
    .await?;

    Ok(links)
}

/// Fetch one link by id, or `None` if it does not exist.
///
/// # Errors
/// Returns [`AppError::Database`] if the query fails.
pub async fn get_link(pool: &PgPool, id: Uuid) -> Result<Option<Link>, AppError> {
    let link = sqlx::query_as!(
        Link,
        r#"
        select id, url, title, description, tags, clicks, created_at, updated_at
        from links
        where id = $1
        "#,
        id
    )
    .fetch_optional(pool)
    .await?;

    Ok(link)
}

/// Case-insensitive substring match across title, url and description, in
/// `sort` order.
///
/// # Errors
/// Returns [`AppError::Database`] if the query fails.
pub async fn search_links(
    pool: &PgPool,
    q: &str,
    limit: i64,
    sort: LinkSort,
) -> Result<Vec<Link>, AppError> {
    let pattern = format!("%{}%", q.trim());

    let links = sqlx::query_as!(
        Link,
        r#"
        select id, url, title, description, tags, clicks, created_at, updated_at
        from links
        where title ilike $1 or url ilike $1 or coalesce(description, '') ilike $1
        order by
            case when $3 = 'title' then lower(title) end asc,
            case when $3 = 'clicks' then clicks end desc,
            created_at desc,
            id
        limit $2
        "#,
        pattern,
        limit.clamp(1, 500),
        sort.as_str()
    )
    .fetch_all(pool)
    .await?;

    Ok(links)
}

/// Insert a new link and return it as stored.
///
/// # Errors
/// Returns [`AppError::Invalid`] if the url is not `http(s)://` (a backstop
/// for callers that skipped [`NewLink::validate`]), or [`AppError::Database`]
/// if the insert fails.
pub async fn create_link(pool: &PgPool, new: &NewLink) -> Result<Link, AppError> {
    validate_url(&new.url)?;
    let link = sqlx::query_as!(
        Link,
        r#"
        insert into links (url, title, description, tags)
        values ($1, $2, $3, $4)
        returning id, url, title, description, tags, clicks, created_at, updated_at
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
///
/// Returns `None` if no link has that id.
///
/// # Errors
/// Returns [`AppError::Invalid`] if a supplied url is not `http(s)://` (a
/// backstop for callers that skipped [`UpdateLink::validate`]), or
/// [`AppError::Database`] if the update fails.
pub async fn update_link(
    pool: &PgPool,
    id: Uuid,
    up: &UpdateLink,
) -> Result<Option<Link>, AppError> {
    if let Some(url) = up.url.as_deref() {
        validate_url(url)?;
    }
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
        returning id, url, title, description, tags, clicks, created_at, updated_at
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
///
/// # Errors
/// Returns [`AppError::Database`] if the delete fails.
pub async fn delete_link(pool: &PgPool, id: Uuid) -> Result<bool, AppError> {
    let result = sqlx::query!("delete from links where id = $1", id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}

/// Count one click on a link. Returns whether the link exists. Deliberately
/// leaves `updated_at` alone: opening a link is not editing it.
///
/// # Errors
/// Returns [`AppError::Database`] if the update fails.
pub async fn record_click(pool: &PgPool, id: Uuid) -> Result<bool, AppError> {
    let result = sqlx::query!("update links set clicks = clicks + 1 where id = $1", id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}
