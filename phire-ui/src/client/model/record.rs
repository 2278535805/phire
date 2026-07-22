use super::{Chart, Object, User};
use chrono::{DateTime, Utc};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct Record {
    pub id: String,
    pub owner_id: i32,
    pub chart_id: String,
    pub score: i32,
    pub accuracy: f64,
    pub is_full_combo: bool,
    pub max_combo: i32,
    pub perfect: i32,
    pub good_early: i32,
    pub good_late: i32,
    pub bad: i32,
    pub miss: i32,
    pub std_deviation: f64,
    pub rks: f64,
    pub perfect_judgment: i32,
    pub good_judgment: i32,
    pub device_info: Option<String>,
    pub application_id: String,
    #[serde(default)]
    pub position: Option<i32>,
    pub date_created: DateTime<Utc>,
}
impl Object for Record {
    const QUERY_PATH: &'static str = "records";

    fn id(&self) -> String {
        self.id.clone()
    }
}
