use super::{Object, Ptr, User};
use crate::data::BriefChartInfo;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TagInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub normalized_name: Option<String>,
}

fn deserialize_tags<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let tags: Vec<serde_json::Value> = Vec::deserialize(deserializer)?;
    Ok(tags
        .into_iter()
        .map(|v| match v {
            serde_json::Value::String(s) => s,
            serde_json::Value::Object(ref map) => {
                map.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string()
            }
            _ => String::new(),
        })
        .collect())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartAssetInfo {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub asset_type: u8,
    pub file: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Song {
    pub id: String,
    pub title: String,
    pub edition_type: u8,
    pub edition: Option<String>,
    pub author_name: Option<String>,
    pub file: Option<String>,
    pub file_checksum: Option<String>,
    pub illustration: Option<String>,
    pub illustrator: Option<String>,
    pub description: Option<String>,
    pub accessibility: u8,
    pub is_hidden: bool,
    pub is_locked: bool,
    pub lyrics: Option<String>,
    pub bpm: f64,
    pub min_bpm: f64,
    pub max_bpm: f64,
    pub offset: i32,
    pub license: Option<String>,
    pub is_original: bool,
    pub duration: Option<String>,
    pub preview_start: Option<String>,
    pub preview_end: Option<String>,
    #[serde(default)]
    pub chart_levels: Vec<ChartLevelInfo>,
    #[serde(default, deserialize_with = "deserialize_tags")]
    pub tags: Vec<String>,
    pub play_count: Option<i64>,
    pub like_count: Option<i32>,
    pub owner_id: Option<i32>,
    #[serde(default)]
    pub date_created: Option<DateTime<Utc>>,
    #[serde(default)]
    pub date_file_updated: Option<DateTime<Utc>>,
    #[serde(default)]
    pub date_updated: Option<DateTime<Utc>>,
    #[serde(default)]
    pub date_liked: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartLevelInfo {
    pub level_type: u8,
    pub count: i32,
}

pub fn parse_author_name(author: &str) -> String {
    if let Some(s) = author.strip_prefix("[PZUser:") {
        let s = s.strip_suffix(":PZRT]").unwrap_or(s);
        s.split(':').nth(1).unwrap_or(s).to_string()
    } else {
        author.to_string()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chart {
    pub id: String,
    pub title: Option<String>,
    pub level_type: u8,
    pub level: String,
    pub difficulty: f32,
    pub format: u8,
    pub file: Option<String>,
    pub file_checksum: Option<String>,
    pub author_name: String,
    pub illustration: Option<String>,
    pub illustrator: Option<String>,
    pub description: Option<String>,
    pub accessibility: u8,
    pub is_hidden: bool,
    pub is_locked: bool,
    pub is_ranked: bool,
    pub note_count: i32,
    pub score: f64,
    pub rating: f64,
    #[serde(default, deserialize_with = "deserialize_tags")]
    pub tags: Vec<String>,
    #[serde(default)]
    pub assets: Vec<ChartAssetInfo>,

    pub song_id: String,
    #[serde(default)]
    pub song: Option<Song>,

    pub owner_id: i32,

    pub rating_on_arrangement: Option<f64>,
    pub rating_on_gameplay: Option<f64>,
    pub rating_on_visual_effects: Option<f64>,
    pub rating_on_creativity: Option<f64>,
    pub rating_on_concord: Option<f64>,
    pub rating_on_impression: Option<f64>,
    pub play_count: Option<i64>,
    pub like_count: Option<i32>,
    #[serde(default)]
    pub date_created: Option<DateTime<Utc>>,
    #[serde(default)]
    pub date_file_updated: Option<DateTime<Utc>>,
    #[serde(default)]
    pub date_updated: Option<DateTime<Utc>>,
    #[serde(default)]
    pub date_liked: Option<DateTime<Utc>>,
    #[serde(default)]
    pub personal_best_score: Option<i32>,
    #[serde(default)]
    pub personal_best_accuracy: Option<f64>,
    #[serde(default)]
    pub personal_best_rks: Option<f64>,
}
impl Object for Chart {
    const QUERY_PATH: &'static str = "charts";

    fn id(&self) -> String {
        self.id.clone()
    }
}

impl Chart {
    pub fn to_info(&self) -> BriefChartInfo {
        let name = self
            .title
            .clone()
            .or_else(|| self.song.as_ref().map(|s| s.title.clone()))
            .unwrap_or_default();
        BriefChartInfo {
            id: Some(0),
            uploader: Some(Ptr::new(self.owner_id.to_string())),
            name,
            level: self.level.clone(),
            difficulty: self.difficulty,
            intro: self.description.clone().unwrap_or_default(),
            charter: parse_author_name(&self.author_name),
            composer: String::new(),
            illustrator: self.illustrator.clone().unwrap_or_default(),
            score_total: 1_000_000,
            created: self.date_created,
            updated: self.date_updated,
            chart_updated: self.date_file_updated,
            has_unlock: false,
        }
    }
}
