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
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
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
        if !(self.url.starts_with("http://") || self.url.starts_with("https://")) {
            return Err(crate::error::AppError::Invalid(
                "url must start with http:// or https://".into(),
            ));
        }
        Ok(())
    }
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
    fn validate_rejects_blank_url_and_title() {
        let mut blank_url = new_link(r#"{"url":"   ","title":"a"}"#);
        assert!(blank_url.validate().is_err());

        let mut blank_title = new_link(r#"{"url":"https://a.dev","title":"   "}"#);
        assert!(blank_title.validate().is_err());
    }

    #[test]
    fn update_omitting_tags_leaves_them_unchanged() {
        // `None` means "leave alone" — it must not be confused with "clear".
        let update: UpdateLink = serde_json::from_str(r#"{"title":"new"}"#).expect("valid");
        assert!(update.tags.is_none());
        assert_eq!(update.title.as_deref(), Some("new"));
    }
}
