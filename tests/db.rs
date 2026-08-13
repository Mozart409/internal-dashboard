//! Direct tests for the data-access contract in `internal_dashboard::db`.
//!
//! These bypass HTTP entirely: every test calls the `db` functions with the
//! pool that `#[sqlx::test]` hands out, so each one owns its own freshly
//! migrated database and nothing here may assume global state.

mod common;

use chrono::{DateTime, TimeDelta, Utc};
use internal_dashboard::db;
use internal_dashboard::models::{Link, NewLink, UpdateLink};
use sqlx::PgPool;
use uuid::Uuid;

// --- helpers ----------------------------------------------------------------

/// A minimal valid link: no description, no tags.
fn new_link(url: &str, title: &str) -> NewLink {
    NewLink {
        url: url.to_owned(),
        title: title.to_owned(),
        description: None,
        tags: Vec::new(),
    }
}

fn with_tags(url: &str, title: &str, tags: &[&str]) -> NewLink {
    NewLink {
        tags: tags.iter().copied().map(String::from).collect(),
        ..new_link(url, title)
    }
}

fn with_description(url: &str, title: &str, description: &str) -> NewLink {
    NewLink {
        description: Some(description.to_owned()),
        ..new_link(url, title)
    }
}

async fn create(pool: &PgPool, new: &NewLink) -> Link {
    db::create_link(pool, new)
        .await
        .expect("create_link should succeed")
}

/// Re-read a row that must still exist, so assertions test what is actually
/// stored rather than what a `returning` clause echoed back.
async fn reload(pool: &PgPool, id: Uuid) -> Link {
    db::get_link(pool, id)
        .await
        .expect("get_link should not error")
        .expect("the row should still exist")
}

async fn update(pool: &PgPool, id: Uuid, up: &UpdateLink) -> Link {
    db::update_link(pool, id, up)
        .await
        .expect("update_link should not error")
        .expect("update_link should find the row")
}

/// Pin `created_at` to an exact instant. Two inserts can otherwise land in the
/// same microsecond, which would make an ordering assertion ambiguous.
async fn set_created_at(pool: &PgPool, id: Uuid, at: DateTime<Utc>) {
    let affected = sqlx::query("update links set created_at = $2 where id = $1")
        .bind(id)
        .bind(at)
        .execute(pool)
        .await
        .expect("backdating created_at should succeed")
        .rows_affected();

    assert_eq!(affected, 1, "backdating should touch exactly one row");
}

fn titles(links: &[Link]) -> Vec<&str> {
    links.iter().map(|l| l.title.as_str()).collect()
}

// --- create_link / get_link -------------------------------------------------

#[sqlx::test]
async fn create_link_persists_every_field(pool: PgPool) {
    let new = NewLink {
        url: "https://doc.rust-lang.org".to_owned(),
        title: "The Rust docs".to_owned(),
        description: Some("Official documentation".to_owned()),
        tags: vec!["rust".to_owned(), "docs".to_owned()],
    };

    let created = create(&pool, &new).await;

    assert_eq!(created.url, new.url, "url should round trip");
    assert_eq!(created.title, new.title, "title should round trip");
    assert_eq!(
        created.description, new.description,
        "description should round trip"
    );
    assert_eq!(created.tags, new.tags, "tags text[] should round trip");
    assert_ne!(created.id, Uuid::nil(), "the database should assign an id");
}

#[sqlx::test]
async fn create_link_stamps_both_timestamps_to_now(pool: PgPool) {
    let created = create(&pool, &new_link("https://a.dev", "a")).await;

    assert_eq!(
        created.created_at, created.updated_at,
        "a freshly created row should have created_at == updated_at"
    );
    assert!(
        (Utc::now() - created.created_at).num_seconds().abs() < 60,
        "created_at should be stamped with the current time, got {}",
        created.created_at
    );
}

#[sqlx::test]
async fn get_link_round_trips_the_created_row(pool: PgPool) {
    let created = create(
        &pool,
        &NewLink {
            description: Some("desc".to_owned()),
            ..with_tags("https://a.dev", "a", &["one", "two"])
        },
    )
    .await;

    let fetched = reload(&pool, created.id).await;

    assert_eq!(
        fetched.id, created.id,
        "get_link should return the same row"
    );
    assert_eq!(
        fetched.url, created.url,
        "url should survive the round trip"
    );
    assert_eq!(
        fetched.title, created.title,
        "title should survive the round trip"
    );
    assert_eq!(
        fetched.description, created.description,
        "description should survive the round trip"
    );
    assert_eq!(
        fetched.tags, created.tags,
        "tags should survive the round trip"
    );
    assert_eq!(
        fetched.created_at, created.created_at,
        "created_at should survive the round trip unmodified"
    );
    assert_eq!(
        fetched.updated_at, created.updated_at,
        "updated_at should survive the round trip unmodified"
    );
}

#[sqlx::test]
async fn get_link_returns_none_for_an_unknown_id(pool: PgPool) {
    let found = db::get_link(&pool, Uuid::new_v4())
        .await
        .expect("a missing id is not an error, it is Ok(None)");

    assert!(found.is_none(), "an unknown id should yield Ok(None)");
}

#[sqlx::test]
async fn get_link_returns_none_after_the_table_has_other_rows(pool: PgPool) {
    create(&pool, &new_link("https://a.dev", "a")).await;

    let found = db::get_link(&pool, Uuid::new_v4())
        .await
        .expect("get_link should not error");

    assert!(
        found.is_none(),
        "an unknown id should yield None even when other rows exist"
    );
}

#[sqlx::test]
async fn create_link_does_not_validate_its_input(pool: PgPool) {
    // Validation lives in `NewLink::validate`, not in the data layer. This
    // pins the layering: db::create_link stores whatever it is handed.
    let created = create(&pool, &new_link("not-a-url", "")).await;

    assert_eq!(created.url, "not-a-url", "db layer stores the url verbatim");
    assert_eq!(created.title, "", "db layer stores an empty title verbatim");
}

// --- list_links -------------------------------------------------------------

#[sqlx::test]
async fn list_links_returns_empty_on_a_fresh_database(pool: PgPool) {
    let links = db::list_links(&pool, None)
        .await
        .expect("list_links should not error");

    assert!(links.is_empty(), "a fresh database should list no links");
}

#[sqlx::test]
async fn list_links_returns_every_link_when_tag_is_none(pool: PgPool) {
    create(&pool, &with_tags("https://a.dev", "a", &["rust"])).await;
    create(&pool, &with_tags("https://b.dev", "b", &["ops"])).await;
    create(&pool, &new_link("https://c.dev", "c")).await;

    let links = db::list_links(&pool, None)
        .await
        .expect("list_links should not error");

    assert_eq!(
        links.len(),
        3,
        "tag = None must not filter anything, got {:?}",
        titles(&links)
    );
}

#[sqlx::test]
async fn list_links_filters_to_links_carrying_the_tag(pool: PgPool) {
    create(&pool, &with_tags("https://a.dev", "a", &["rust", "docs"])).await;
    create(&pool, &with_tags("https://b.dev", "b", &["ops"])).await;
    create(&pool, &with_tags("https://c.dev", "c", &["rust"])).await;
    create(&pool, &new_link("https://d.dev", "d")).await;

    let links = db::list_links(&pool, Some("rust"))
        .await
        .expect("list_links should not error");

    let mut found = titles(&links);
    found.sort_unstable();
    assert_eq!(
        found,
        vec!["a", "c"],
        "only links carrying the tag should be returned"
    );
}

#[sqlx::test]
async fn list_links_tag_filter_is_an_exact_match_not_a_substring(pool: PgPool) {
    create(&pool, &with_tags("https://a.dev", "a", &["rustacean"])).await;

    let links = db::list_links(&pool, Some("rust"))
        .await
        .expect("list_links should not error");

    assert!(
        links.is_empty(),
        "the tag filter uses = any(tags), so 'rust' must not match 'rustacean'"
    );
}

#[sqlx::test]
async fn list_links_is_case_sensitive_on_tags(pool: PgPool) {
    create(&pool, &with_tags("https://a.dev", "a", &["rust"])).await;

    let links = db::list_links(&pool, Some("RUST"))
        .await
        .expect("list_links should not error");

    assert!(
        links.is_empty(),
        "tag matching is exact equality, so it is case sensitive"
    );
}

#[sqlx::test]
async fn list_links_returns_empty_for_a_tag_nobody_has(pool: PgPool) {
    create(&pool, &with_tags("https://a.dev", "a", &["rust"])).await;
    create(&pool, &with_tags("https://b.dev", "b", &["ops"])).await;

    let links = db::list_links(&pool, Some("nobody-has-this"))
        .await
        .expect("an unmatched tag is not an error");

    assert!(
        links.is_empty(),
        "an unmatched tag should give an empty vec, got {:?}",
        titles(&links)
    );
}

#[sqlx::test]
async fn list_links_orders_by_created_at_descending(pool: PgPool) {
    let now = Utc::now();
    let oldest = create(&pool, &new_link("https://a.dev", "oldest")).await;
    let middle = create(&pool, &new_link("https://b.dev", "middle")).await;
    let newest = create(&pool, &new_link("https://c.dev", "newest")).await;

    // Insert order is deliberately not the expected output order.
    set_created_at(&pool, oldest.id, now - TimeDelta::hours(3)).await;
    set_created_at(&pool, newest.id, now - TimeDelta::hours(1)).await;
    set_created_at(&pool, middle.id, now - TimeDelta::hours(2)).await;

    let links = db::list_links(&pool, None)
        .await
        .expect("list_links should not error");

    assert_eq!(
        titles(&links),
        vec!["newest", "middle", "oldest"],
        "list_links must order by created_at desc (newest first)"
    );
}

#[sqlx::test]
async fn list_links_returns_sequential_inserts_newest_first(pool: PgPool) {
    // Same invariant as above, but with timestamps the database assigned
    // itself rather than ones the test forced.
    for title in ["first", "second", "third", "fourth"] {
        create(&pool, &new_link("https://a.dev", title)).await;
        tokio::time::sleep(std::time::Duration::from_millis(3)).await;
    }

    let links = db::list_links(&pool, None)
        .await
        .expect("list_links should not error");

    assert_eq!(
        titles(&links),
        vec!["fourth", "third", "second", "first"],
        "the most recently inserted link must come first"
    );
}

#[sqlx::test]
async fn list_links_orders_by_created_at_descending_when_filtering_by_tag(pool: PgPool) {
    let now = Utc::now();
    let old = create(&pool, &with_tags("https://a.dev", "old", &["rust"])).await;
    let new = create(&pool, &with_tags("https://b.dev", "new", &["rust"])).await;
    create(&pool, &with_tags("https://c.dev", "other", &["ops"])).await;

    set_created_at(&pool, old.id, now - TimeDelta::hours(2)).await;
    set_created_at(&pool, new.id, now - TimeDelta::hours(1)).await;

    let links = db::list_links(&pool, Some("rust"))
        .await
        .expect("list_links should not error");

    assert_eq!(
        titles(&links),
        vec!["new", "old"],
        "the tag-filtered listing must still be newest first"
    );
}

// --- search_links -----------------------------------------------------------

#[sqlx::test]
async fn search_links_matches_on_title(pool: PgPool) {
    create(
        &pool,
        &new_link("https://example.com/x", "Quarterly report"),
    )
    .await;
    create(&pool, &new_link("https://example.com/y", "Something else")).await;

    let found = db::search_links(&pool, "quarterly", 50)
        .await
        .expect("search_links should not error");

    assert_eq!(
        titles(&found),
        vec!["Quarterly report"],
        "search should match on the title column"
    );
}

#[sqlx::test]
async fn search_links_matches_on_url(pool: PgPool) {
    create(
        &pool,
        &new_link("https://grafana.internal/d/abc", "Dashboards"),
    )
    .await;
    create(&pool, &new_link("https://example.com/y", "Something else")).await;

    let found = db::search_links(&pool, "grafana", 50)
        .await
        .expect("search_links should not error");

    assert_eq!(
        titles(&found),
        vec!["Dashboards"],
        "search should match on the url column"
    );
}

#[sqlx::test]
async fn search_links_matches_on_description(pool: PgPool) {
    create(
        &pool,
        &with_description("https://a.dev", "Runbook", "how to restart the ingester"),
    )
    .await;
    create(&pool, &new_link("https://b.dev", "Something else")).await;

    let found = db::search_links(&pool, "ingester", 50)
        .await
        .expect("search_links should not error");

    assert_eq!(
        titles(&found),
        vec!["Runbook"],
        "search should match on the description column"
    );
}

#[sqlx::test]
async fn search_links_tolerates_a_null_description(pool: PgPool) {
    // `coalesce(description, '')` is what keeps a null description from
    // dropping the row out of an otherwise matching search.
    create(&pool, &new_link("https://a.dev", "no description here")).await;

    let found = db::search_links(&pool, "description", 50)
        .await
        .expect("search_links should not error");

    assert_eq!(
        found.len(),
        1,
        "a row with a null description must still be searchable by title"
    );
}

#[sqlx::test]
async fn search_links_is_case_insensitive(pool: PgPool) {
    create(
        &pool,
        &new_link("https://Example.COM/Path", "MiXeD CaSe TiTlE"),
    )
    .await;

    for query in ["mixed", "MIXED", "MiXeD", "example.com", "EXAMPLE.COM"] {
        let found = db::search_links(&pool, query, 50)
            .await
            .expect("search_links should not error");
        assert_eq!(
            found.len(),
            1,
            "search uses ilike, so {query:?} should match regardless of case"
        );
    }
}

#[sqlx::test]
async fn search_links_matches_a_substring_anywhere(pool: PgPool) {
    create(&pool, &new_link("https://a.dev", "prometheus alerting")).await;

    let found = db::search_links(&pool, "methe", 50)
        .await
        .expect("search_links should not error");

    assert_eq!(
        found.len(),
        1,
        "the query is wrapped in % on both sides, so it matches mid-word"
    );
}

#[sqlx::test]
async fn search_links_trims_the_query(pool: PgPool) {
    create(&pool, &new_link("https://a.dev", "prometheus")).await;

    let found = db::search_links(&pool, "   prometheus   ", 50)
        .await
        .expect("search_links should not error");

    assert_eq!(
        found.len(),
        1,
        "surrounding whitespace is trimmed before the pattern is built"
    );
}

#[sqlx::test]
async fn search_links_returns_empty_when_nothing_matches(pool: PgPool) {
    create(&pool, &new_link("https://a.dev", "alpha")).await;
    create(&pool, &new_link("https://b.dev", "beta")).await;

    let found = db::search_links(&pool, "no-such-thing-anywhere", 50)
        .await
        .expect("a query with no hits is not an error");

    assert!(
        found.is_empty(),
        "a query with no hits should give an empty vec"
    );
}

#[sqlx::test]
async fn search_links_orders_by_created_at_descending(pool: PgPool) {
    let now = Utc::now();
    let old = create(&pool, &new_link("https://a.dev", "match old")).await;
    let new = create(&pool, &new_link("https://b.dev", "match new")).await;

    set_created_at(&pool, old.id, now - TimeDelta::hours(2)).await;
    set_created_at(&pool, new.id, now - TimeDelta::hours(1)).await;

    let found = db::search_links(&pool, "match", 50)
        .await
        .expect("search_links should not error");

    assert_eq!(
        titles(&found),
        vec!["match new", "match old"],
        "search results must be newest first"
    );
}

#[sqlx::test]
async fn search_links_respects_the_limit(pool: PgPool) {
    for i in 0..5 {
        create(&pool, &new_link("https://a.dev", &format!("match {i}"))).await;
    }

    let found = db::search_links(&pool, "match", 2)
        .await
        .expect("search_links should not error");

    assert_eq!(found.len(), 2, "search should return at most `limit` rows");
}

#[sqlx::test]
async fn search_links_returns_fewer_rows_than_the_limit_allows(pool: PgPool) {
    create(&pool, &new_link("https://a.dev", "match")).await;

    let found = db::search_links(&pool, "match", 100)
        .await
        .expect("search_links should not error");

    assert_eq!(found.len(), 1, "the limit is a ceiling, not a target");
}

#[sqlx::test]
async fn search_links_clamps_a_zero_limit_up_to_one(pool: PgPool) {
    for i in 0..3 {
        create(&pool, &new_link("https://a.dev", &format!("match {i}"))).await;
    }

    let found = db::search_links(&pool, "match", 0)
        .await
        .expect("limit 0 must not error — it is clamped, not rejected");

    assert_eq!(found.len(), 1, "a limit of 0 should be clamped up to 1");
}

#[sqlx::test]
async fn search_links_clamps_a_negative_limit_up_to_one(pool: PgPool) {
    for i in 0..3 {
        create(&pool, &new_link("https://a.dev", &format!("match {i}"))).await;
    }

    let found = db::search_links(&pool, "match", -9999)
        .await
        .expect("a negative limit must not reach Postgres, which would reject it");

    assert_eq!(found.len(), 1, "a negative limit should be clamped up to 1");
}

#[sqlx::test]
async fn search_links_clamps_a_huge_limit_down_to_five_hundred(pool: PgPool) {
    // Bulk-insert in one statement: 600 round trips would dominate the runtime.
    sqlx::query(
        "insert into links (url, title)
         select 'https://bulk.dev/' || i, 'bulk ' || i
         from generate_series(1, 600) as i",
    )
    .execute(&pool)
    .await
    .expect("bulk insert should succeed");

    let found = db::search_links(&pool, "bulk", 9999)
        .await
        .expect("an oversized limit must not error — it is clamped");

    assert_eq!(
        found.len(),
        500,
        "the limit should be clamped down to the 500-row ceiling"
    );
}

#[sqlx::test]
async fn search_links_treats_an_empty_query_as_match_everything(pool: PgPool) {
    // Documenting current behaviour: the query is interpolated into a `like`
    // pattern with no escaping, so "" becomes '%%' and matches every row.
    create(&pool, &new_link("https://a.dev", "a")).await;
    create(&pool, &new_link("https://b.dev", "b")).await;

    let found = db::search_links(&pool, "", 50)
        .await
        .expect("search_links should not error");

    assert_eq!(found.len(), 2, "an empty query currently matches every row");
}

#[sqlx::test]
async fn search_links_does_not_escape_like_metacharacters(pool: PgPool) {
    // Documenting current behaviour, not endorsing it: `%` and `_` from the
    // caller reach Postgres as wildcards instead of literal characters.
    create(&pool, &new_link("https://a.dev", "alpha")).await;
    create(&pool, &new_link("https://b.dev", "beta")).await;

    let wildcard = db::search_links(&pool, "%", 50)
        .await
        .expect("search_links should not error");
    assert_eq!(
        wildcard.len(),
        2,
        "a bare % is treated as a wildcard, matching everything"
    );

    let underscore = db::search_links(&pool, "alph_", 50)
        .await
        .expect("search_links should not error");
    assert_eq!(
        underscore.len(),
        1,
        "a bare _ is treated as a single-character wildcard"
    );
}

// --- update_link ------------------------------------------------------------

#[sqlx::test]
async fn update_link_updates_the_url(pool: PgPool) {
    let created = create(&pool, &new_link("https://before.dev", "title")).await;

    let updated = update(
        &pool,
        created.id,
        &UpdateLink {
            url: Some("https://after.dev".to_owned()),
            ..UpdateLink::default()
        },
    )
    .await;

    assert_eq!(updated.url, "https://after.dev", "url should be updated");
    assert_eq!(
        reload(&pool, created.id).await.url,
        "https://after.dev",
        "the new url should be persisted, not just returned"
    );
}

#[sqlx::test]
async fn update_link_updates_the_title(pool: PgPool) {
    let created = create(&pool, &new_link("https://a.dev", "before")).await;

    let updated = update(
        &pool,
        created.id,
        &UpdateLink {
            title: Some("after".to_owned()),
            ..UpdateLink::default()
        },
    )
    .await;

    assert_eq!(updated.title, "after", "title should be updated");
    assert_eq!(
        reload(&pool, created.id).await.title,
        "after",
        "the new title should be persisted"
    );
}

#[sqlx::test]
async fn update_link_updates_the_description(pool: PgPool) {
    let created = create(&pool, &with_description("https://a.dev", "a", "before")).await;

    let updated = update(
        &pool,
        created.id,
        &UpdateLink {
            description: Some("after".to_owned()),
            ..UpdateLink::default()
        },
    )
    .await;

    assert_eq!(
        updated.description.as_deref(),
        Some("after"),
        "description should be updated"
    );
}

#[sqlx::test]
async fn update_link_sets_a_description_that_was_previously_null(pool: PgPool) {
    let created = create(&pool, &new_link("https://a.dev", "a")).await;
    assert!(
        created.description.is_none(),
        "precondition: the link starts with no description"
    );

    let updated = update(
        &pool,
        created.id,
        &UpdateLink {
            description: Some("now it has one".to_owned()),
            ..UpdateLink::default()
        },
    )
    .await;

    assert_eq!(
        updated.description.as_deref(),
        Some("now it has one"),
        "a null description should be settable"
    );
}

#[sqlx::test]
async fn update_link_updates_the_tags(pool: PgPool) {
    let created = create(&pool, &with_tags("https://a.dev", "a", &["old"])).await;

    let updated = update(
        &pool,
        created.id,
        &UpdateLink {
            tags: Some(vec!["new".to_owned(), "shiny".to_owned()]),
            ..UpdateLink::default()
        },
    )
    .await;

    assert_eq!(
        updated.tags,
        vec!["new", "shiny"],
        "tags should be replaced wholesale, not merged"
    );
}

#[sqlx::test]
async fn update_link_can_clear_tags_with_an_empty_vec(pool: PgPool) {
    let created = create(&pool, &with_tags("https://a.dev", "a", &["rust", "docs"])).await;

    let updated = update(
        &pool,
        created.id,
        &UpdateLink {
            tags: Some(Vec::new()),
            ..UpdateLink::default()
        },
    )
    .await;

    assert!(
        updated.tags.is_empty(),
        "Some(empty vec) means 'clear the tags', unlike None which means 'leave alone'"
    );
    assert!(
        reload(&pool, created.id).await.tags.is_empty(),
        "the cleared tags should be persisted"
    );
}

#[sqlx::test]
async fn update_link_with_only_a_title_leaves_every_other_field_unchanged(pool: PgPool) {
    // The highest-value invariant in this file: `None` means "leave unchanged",
    // and must never be confused with "set to null".
    let created = create(
        &pool,
        &NewLink {
            url: "https://keep.dev".to_owned(),
            title: "before".to_owned(),
            description: Some("a description worth keeping".to_owned()),
            tags: vec!["rust".to_owned(), "docs".to_owned()],
        },
    )
    .await;

    let updated = update(
        &pool,
        created.id,
        &UpdateLink {
            title: Some("after".to_owned()),
            ..UpdateLink::default()
        },
    )
    .await;

    assert_eq!(
        updated.title, "after",
        "the supplied title should be applied"
    );
    assert_eq!(
        updated.url, "https://keep.dev",
        "omitting url must leave it unchanged, not null it"
    );
    assert_eq!(
        updated.description.as_deref(),
        Some("a description worth keeping"),
        "omitting description must leave it unchanged, not null it"
    );
    assert_eq!(
        updated.tags,
        vec!["rust", "docs"],
        "omitting tags must leave the array unchanged, not clear it"
    );

    let reloaded = reload(&pool, created.id).await;
    assert_eq!(
        reloaded.url, updated.url,
        "the untouched url must be what is actually stored"
    );
    assert_eq!(
        reloaded.description, updated.description,
        "the untouched description must be what is actually stored"
    );
    assert_eq!(
        reloaded.tags, updated.tags,
        "the untouched tags must be what is actually stored"
    );
}

#[sqlx::test]
async fn update_link_with_only_tags_leaves_the_text_fields_unchanged(pool: PgPool) {
    let created = create(
        &pool,
        &NewLink {
            url: "https://keep.dev".to_owned(),
            title: "keep me".to_owned(),
            description: Some("keep me too".to_owned()),
            tags: vec!["old".to_owned()],
        },
    )
    .await;

    let updated = update(
        &pool,
        created.id,
        &UpdateLink {
            tags: Some(vec!["fresh".to_owned()]),
            ..UpdateLink::default()
        },
    )
    .await;

    assert_eq!(updated.tags, vec!["fresh"], "tags should be replaced");
    assert_eq!(
        updated.url, "https://keep.dev",
        "omitting url must leave it unchanged"
    );
    assert_eq!(
        updated.title, "keep me",
        "omitting title must leave it unchanged"
    );
    assert_eq!(
        updated.description.as_deref(),
        Some("keep me too"),
        "omitting description must leave it unchanged"
    );
}

#[sqlx::test]
async fn update_link_with_no_fields_at_all_changes_nothing_but_updated_at(pool: PgPool) {
    let created = create(
        &pool,
        &NewLink {
            description: Some("desc".to_owned()),
            ..with_tags("https://a.dev", "a", &["rust"])
        },
    )
    .await;

    let updated = update(&pool, created.id, &UpdateLink::default()).await;

    assert_eq!(
        updated.url, created.url,
        "an empty update must not touch url"
    );
    assert_eq!(
        updated.title, created.title,
        "an empty update must not touch title"
    );
    assert_eq!(
        updated.description, created.description,
        "an empty update must not touch description"
    );
    assert_eq!(
        updated.tags, created.tags,
        "an empty update must not touch tags"
    );
    assert_eq!(
        updated.created_at, created.created_at,
        "an empty update must not touch created_at"
    );
}

#[sqlx::test]
async fn update_link_moves_updated_at_forward_but_leaves_created_at_alone(pool: PgPool) {
    let created = create(&pool, &new_link("https://a.dev", "before")).await;
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;

    let updated = update(
        &pool,
        created.id,
        &UpdateLink {
            title: Some("after".to_owned()),
            ..UpdateLink::default()
        },
    )
    .await;

    assert_eq!(
        updated.created_at, created.created_at,
        "created_at must never move on update"
    );
    assert!(
        updated.updated_at > created.updated_at,
        "updated_at must move forward: {} should be later than {}",
        updated.updated_at,
        created.updated_at
    );
    assert!(
        updated.updated_at > updated.created_at,
        "after an update, updated_at should be later than created_at"
    );
}

#[sqlx::test]
async fn update_link_cannot_null_a_description_only_blank_it(pool: PgPool) {
    // `description = coalesce($4, description)` means a null argument is
    // "unchanged", so there is no value of UpdateLink that clears the column.
    // The closest a caller can get is the empty string.
    let created = create(&pool, &with_description("https://a.dev", "a", "original")).await;

    let updated = update(
        &pool,
        created.id,
        &UpdateLink {
            description: Some(String::new()),
            ..UpdateLink::default()
        },
    )
    .await;

    assert_eq!(
        updated.description.as_deref(),
        Some(""),
        "an empty string should be stored as an empty string, not as null"
    );
}

#[sqlx::test]
async fn update_link_updates_only_the_targeted_row(pool: PgPool) {
    let target = create(&pool, &new_link("https://a.dev", "target")).await;
    let bystander = create(&pool, &new_link("https://b.dev", "bystander")).await;

    update(
        &pool,
        target.id,
        &UpdateLink {
            title: Some("changed".to_owned()),
            ..UpdateLink::default()
        },
    )
    .await;

    let untouched = reload(&pool, bystander.id).await;
    assert_eq!(
        untouched.title, "bystander",
        "updating one row must not touch another"
    );
    assert_eq!(
        untouched.updated_at, bystander.updated_at,
        "updating one row must not bump another row's updated_at"
    );
}

#[sqlx::test]
async fn update_link_returns_none_for_an_unknown_id(pool: PgPool) {
    let missing = db::update_link(
        &pool,
        Uuid::new_v4(),
        &UpdateLink {
            title: Some("nobody".to_owned()),
            ..UpdateLink::default()
        },
    )
    .await
    .expect("updating a missing id is not an error, it is Ok(None)");

    assert!(
        missing.is_none(),
        "update_link should yield Ok(None) when the id does not exist"
    );
}

#[sqlx::test]
async fn update_link_can_change_every_field_at_once(pool: PgPool) {
    let created = create(
        &pool,
        &NewLink {
            description: Some("old desc".to_owned()),
            ..with_tags("https://old.dev", "old title", &["old"])
        },
    )
    .await;

    let updated = update(
        &pool,
        created.id,
        &UpdateLink {
            url: Some("https://new.dev".to_owned()),
            title: Some("new title".to_owned()),
            description: Some("new desc".to_owned()),
            tags: Some(vec!["new".to_owned()]),
        },
    )
    .await;

    assert_eq!(updated.url, "https://new.dev", "url should be updated");
    assert_eq!(updated.title, "new title", "title should be updated");
    assert_eq!(
        updated.description.as_deref(),
        Some("new desc"),
        "description should be updated"
    );
    assert_eq!(updated.tags, vec!["new"], "tags should be updated");
    assert_eq!(
        updated.id, created.id,
        "the id must be stable across an update"
    );
}

// --- delete_link ------------------------------------------------------------

#[sqlx::test]
async fn delete_link_returns_true_and_really_removes_the_row(pool: PgPool) {
    let created = create(&pool, &new_link("https://a.dev", "a")).await;

    let deleted = db::delete_link(&pool, created.id)
        .await
        .expect("delete_link should not error");

    assert!(deleted, "deleting an existing row should report true");
    assert!(
        db::get_link(&pool, created.id)
            .await
            .expect("get_link should not error")
            .is_none(),
        "the row should really be gone after a delete"
    );
}

#[sqlx::test]
async fn delete_link_returns_false_for_an_unknown_id(pool: PgPool) {
    let deleted = db::delete_link(&pool, Uuid::new_v4())
        .await
        .expect("deleting a missing id is not an error");

    assert!(
        !deleted,
        "deleting a row that never existed should be false"
    );
}

#[sqlx::test]
async fn delete_link_is_not_idempotent_in_its_return_value(pool: PgPool) {
    let created = create(&pool, &new_link("https://a.dev", "a")).await;

    let first = db::delete_link(&pool, created.id)
        .await
        .expect("delete_link should not error");
    let second = db::delete_link(&pool, created.id)
        .await
        .expect("delete_link should not error");

    assert!(first, "the first delete removed a row");
    assert!(!second, "the second delete removed nothing, so it is false");
}

#[sqlx::test]
async fn delete_link_removes_only_the_targeted_row(pool: PgPool) {
    let doomed = create(&pool, &new_link("https://a.dev", "doomed")).await;
    let survivor = create(&pool, &new_link("https://b.dev", "survivor")).await;

    assert!(
        db::delete_link(&pool, doomed.id)
            .await
            .expect("delete_link should not error"),
        "precondition: the targeted row was deleted"
    );

    let remaining = db::list_links(&pool, None)
        .await
        .expect("list_links should not error");
    assert_eq!(
        titles(&remaining),
        vec!["survivor"],
        "only the targeted row should be deleted"
    );
    assert_eq!(
        reload(&pool, survivor.id).await.id,
        survivor.id,
        "the surviving row should still be fetchable by id"
    );
}

// --- tags text[] round trips ------------------------------------------------

#[sqlx::test]
async fn empty_tags_round_trip_as_an_empty_array(pool: PgPool) {
    let created = create(&pool, &with_tags("https://a.dev", "a", &[])).await;

    assert!(
        created.tags.is_empty(),
        "an empty tag list should come back empty, not as [\"\"]"
    );
    assert!(
        reload(&pool, created.id).await.tags.is_empty(),
        "an empty tag array should survive a re-read"
    );
}

#[sqlx::test]
async fn many_tags_round_trip_in_order(pool: PgPool) {
    let tags = ["zulu", "alpha", "mike", "bravo", "yankee"];
    let created = create(&pool, &with_tags("https://a.dev", "a", &tags)).await;

    assert_eq!(
        created.tags, tags,
        "text[] should preserve the tags and their order exactly as supplied"
    );
    assert_eq!(
        reload(&pool, created.id).await.tags,
        tags,
        "the tag order should survive a re-read"
    );
}

#[sqlx::test]
async fn duplicate_tags_are_stored_verbatim_by_the_db_layer(pool: PgPool) {
    // Deduplication happens in `NewLink` deserialization, not in db.rs.
    let created = create(&pool, &with_tags("https://a.dev", "a", &["rust", "rust"])).await;

    assert_eq!(
        created.tags,
        vec!["rust", "rust"],
        "the db layer should not silently deduplicate tags"
    );
}

#[sqlx::test]
async fn tags_round_trip_with_array_literal_metacharacters(pool: PgPool) {
    // Commas, braces, quotes and backslashes are all significant in Postgres'
    // textual array syntax; the binary protocol must carry them intact.
    let tags = ["a,b", "{braced}", "with \"quotes\"", "back\\slash", "  "];
    let created = create(&pool, &with_tags("https://a.dev", "a", &tags)).await;

    assert_eq!(
        created.tags, tags,
        "array metacharacters must survive the round trip untouched"
    );
    assert_eq!(
        reload(&pool, created.id).await.tags,
        tags,
        "array metacharacters must survive a re-read"
    );
}

#[sqlx::test]
async fn a_tag_containing_a_comma_is_filterable_as_one_tag(pool: PgPool) {
    create(&pool, &with_tags("https://a.dev", "a", &["a,b"])).await;

    let matched = db::list_links(&pool, Some("a,b"))
        .await
        .expect("list_links should not error");
    let unmatched = db::list_links(&pool, Some("a"))
        .await
        .expect("list_links should not error");

    assert_eq!(
        matched.len(),
        1,
        "the whole comma-containing string is a single tag"
    );
    assert!(
        unmatched.is_empty(),
        "'a' is not a tag here — the tag is the literal string 'a,b'"
    );
}

#[sqlx::test]
async fn tags_round_trip_unicode(pool: PgPool) {
    let tags = ["日本語", "emoji-\u{1f980}", "café"];
    let created = create(&pool, &with_tags("https://a.dev", "a", &tags)).await;

    assert_eq!(created.tags, tags, "unicode tags should round trip intact");

    let matched = db::list_links(&pool, Some("日本語"))
        .await
        .expect("list_links should not error");
    assert_eq!(matched.len(), 1, "a unicode tag should be filterable");
}
