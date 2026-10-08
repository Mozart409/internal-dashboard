use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// A curated link.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, sqlx::FromRow)]
pub struct Link {
    pub id: Uuid,
    /// Target URL.
    pub url: String,
    /// Human-readable title.
    pub title: String,
    /// Optional longer description.
    pub description: Option<String>,
    /// Free-form tags used for filtering.
    pub tags: Vec<String>,
    /// How often the link has been opened from the UI.
    pub clicks: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Order of a link listing. Every surface accepts it; `newest` is the default.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    ToSchema,
    rmcp::schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum LinkSort {
    /// Most recently created first.
    #[default]
    Newest,
    /// Alphabetical by title, case-insensitive.
    Title,
    /// Most opened first, newest breaking ties.
    Clicks,
}

impl LinkSort {
    /// Every variant, in the order the UI toggle shows them.
    pub const ALL: [LinkSort; 3] = [LinkSort::Newest, LinkSort::Title, LinkSort::Clicks];

    /// The wire name, as used in query strings, the cookie and SQL.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            LinkSort::Newest => "newest",
            LinkSort::Title => "title",
            LinkSort::Clicks => "clicks",
        }
    }

    /// Inverse of [`LinkSort::as_str`].
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|sort| sort.as_str() == s)
    }

    /// Label for the UI toggle.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            LinkSort::Newest => "Newest",
            LinkSort::Title => "A–Z",
            LinkSort::Clicks => "Most clicked",
        }
    }
}

/// Payload for creating a link.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct NewLink {
    #[schema(example = "https://example.com")]
    pub url: String,
    #[schema(example = "Example")]
    pub title: String,
    pub description: Option<String>,
    #[serde(default, deserialize_with = "de_tags")]
    pub tags: Vec<String>,
}

/// Payload for updating a link. Absent fields are left unchanged.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub struct UpdateLink {
    pub url: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    #[serde(default, deserialize_with = "de_opt_tags")]
    pub tags: Option<Vec<String>>,
}

impl NewLink {
    /// Reject empty required fields and normalise whitespace.
    ///
    /// # Errors
    /// Returns [`AppError::Invalid`](crate::error::AppError::Invalid) if the
    /// url or title is blank, or the url is not `http://` or `https://`.
    pub fn validate(&mut self) -> Result<(), crate::error::AppError> {
        self.url = self.url.trim().to_string();
        self.title = self.title.trim().to_string();

        if self.url.is_empty() {
            return Err(crate::error::AppError::Invalid("url is required".into()));
        }
        if self.title.is_empty() {
            return Err(crate::error::AppError::Invalid("title is required".into()));
        }
        validate_url(&self.url)
    }
}

impl UpdateLink {
    /// The update counterpart of [`NewLink::validate`]: the fields being set
    /// are trimmed and checked by the same rules, absent ones are left alone.
    ///
    /// # Errors
    /// Returns [`AppError::Invalid`](crate::error::AppError::Invalid) if a
    /// supplied url or title is blank, or the url is not `http://` or
    /// `https://`.
    pub fn validate(&mut self) -> Result<(), crate::error::AppError> {
        if let Some(url) = &mut self.url {
            *url = url.trim().to_string();
            if url.is_empty() {
                return Err(crate::error::AppError::Invalid(
                    "url must not be blank".into(),
                ));
            }
            validate_url(url)?;
        }
        if let Some(title) = &mut self.title {
            *title = title.trim().to_string();
            if title.is_empty() {
                return Err(crate::error::AppError::Invalid(
                    "title must not be blank".into(),
                ));
            }
        }
        Ok(())
    }
}

/// The one url rule every write path shares: an allow-list of `http://` and
/// `https://`. The stored url becomes an anchor's `href`, and HTML escaping
/// does nothing about the scheme, so `javascript:`, `data:` and the like must
/// never be stored.
///
/// # Errors
/// Returns [`AppError::Invalid`](crate::error::AppError::Invalid) for any
/// other scheme.
pub fn validate_url(url: &str) -> Result<(), crate::error::AppError> {
    if is_http_url(url) {
        Ok(())
    } else {
        Err(crate::error::AppError::Invalid(
            "url must start with http:// or https://".into(),
        ))
    }
}

/// Whether `url` is `http://` or `https://`, the only schemes rendered as
/// links.
#[must_use]
pub fn is_http_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

/// HTML forms send tags as one comma-separated field; JSON sends an array.
/// Accept either.
fn de_tags<'de, D>(d: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(de_opt_tags(d)?.unwrap_or_default())
}

pub(crate) fn de_opt_tags<'de, D>(d: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Tags {
        List(Vec<String>),
        Csv(String),
    }

    Ok(match Option::<Tags>::deserialize(d)? {
        None => None,
        Some(Tags::List(v)) => Some(clean(v)),
        Some(Tags::Csv(s)) => Some(clean(s.split(',').map(str::to_string).collect())),
    })
}

fn clean(tags: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = tags
        .into_iter()
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_link(json: &str) -> NewLink {
        serde_json::from_str(json).expect("valid NewLink")
    }

    #[test]
    fn tags_accept_a_json_array() {
        let link = new_link(r#"{"url":"https://a.dev","title":"a","tags":["Rust","docs"]}"#);
        assert_eq!(link.tags, vec!["docs", "rust"]);
    }

    #[test]
    fn tags_accept_a_comma_separated_form_field() {
        // This is the shape an HTML form submits.
        let link = new_link(r#"{"url":"https://a.dev","title":"a","tags":" Rust , docs "}"#);
        assert_eq!(link.tags, vec!["docs", "rust"]);
    }

    #[test]
    fn tags_are_lowercased_sorted_deduped_and_blanks_dropped() {
        let link = new_link(r#"{"url":"https://a.dev","title":"a","tags":"b,,A, a ,B"}"#);
        assert_eq!(link.tags, vec!["a", "b"]);
    }

    #[test]
    fn tags_default_to_empty_when_omitted() {
        let link = new_link(r#"{"url":"https://a.dev","title":"a"}"#);
        assert!(link.tags.is_empty());
    }

    #[test]
    fn validate_trims_and_accepts_http_urls() {
        let mut link = new_link(r#"{"url":"  https://a.dev  ","title":"  a  "}"#);
        link.validate().expect("should be valid");
        assert_eq!(link.url, "https://a.dev");
        assert_eq!(link.title, "a");
    }

    #[test]
    fn validate_rejects_non_http_schemes() {
        let mut link = new_link(r#"{"url":"ftp://a.dev","title":"a"}"#);
        assert!(link.validate().is_err());
    }

    #[test]
    fn validate_rejects_script_and_data_schemes() {
        for url in [
            "javascript:alert(1)",
            "JavaScript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "vbscript:msgbox(1)",
        ] {
            let mut link = NewLink {
                url: url.into(),
                title: "a".into(),
                description: None,
                tags: vec![],
            };
            assert!(link.validate().is_err(), "{url} must be rejected");
        }
    }

    fn update(json: &str) -> UpdateLink {
        serde_json::from_str(json).expect("valid UpdateLink")
    }

    #[test]
    fn update_validate_rejects_a_non_http_url() {
        for url in ["javascript:alert(1)", "data:text/html,x", "  ftp://a.dev  "] {
            let mut up = UpdateLink {
                url: Some(url.into()),
                ..UpdateLink::default()
            };
            assert!(up.validate().is_err(), "{url} must be rejected");
        }
    }

    #[test]
    fn update_validate_trims_the_fields_it_sets_and_ignores_absent_ones() {
        let mut up = update(r#"{"url":"  https://a.dev  ","title":"  a  "}"#);
        up.validate().expect("should be valid");
        assert_eq!(up.url.as_deref(), Some("https://a.dev"));
        assert_eq!(up.title.as_deref(), Some("a"));

        let mut tags_only = update(r#"{"tags":["x"]}"#);
        tags_only.validate().expect("no url or title to check");
    }

    #[test]
    fn update_validate_rejects_blank_url_and_title() {
        assert!(update(r#"{"url":"   "}"#).validate().is_err());
        assert!(update(r#"{"title":"   "}"#).validate().is_err());
    }

    #[test]
    fn validate_rejects_blank_url_and_title() {
        let mut blank_url = new_link(r#"{"url":"   ","title":"a"}"#);
        assert!(blank_url.validate().is_err());

        let mut blank_title = new_link(r#"{"url":"https://a.dev","title":"   "}"#);
        assert!(blank_title.validate().is_err());
    }

    #[test]
    fn sort_parse_round_trips_and_rejects_unknown() {
        for sort in LinkSort::ALL {
            assert_eq!(LinkSort::parse(sort.as_str()), Some(sort));
        }
        assert_eq!(LinkSort::parse("oldest"), None);
    }

    #[test]
    fn sort_deserializes_from_its_wire_name() {
        let sort: LinkSort = serde_json::from_str(r#""clicks""#).expect("valid");
        assert_eq!(sort, LinkSort::Clicks);
    }

    #[test]
    fn update_omitting_tags_leaves_them_unchanged() {
        // `None` means "leave alone" — it must not be confused with "clear".
        let update: UpdateLink = serde_json::from_str(r#"{"title":"new"}"#).expect("valid");
        assert!(update.tags.is_none());
        assert_eq!(update.title.as_deref(), Some("new"));
    }
}
