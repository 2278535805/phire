use super::Object;
use chrono::{DateTime, Utc};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct Event {
    pub id: String,
    pub owner_id: i32,
    pub title: String,
    pub illustration: Option<String>,
    pub date_start: Option<DateTime<Utc>>,
    pub date_end: Option<DateTime<Utc>>,
}
impl Object for Event {
    const QUERY_PATH: &'static str = "events";

    fn id(&self) -> String {
        self.id.clone()
    }
}
