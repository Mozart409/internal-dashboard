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

fn de_opt_tags<'de, D>(d: D) -> Result<Option<Vec<String>>, D::Error>
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
