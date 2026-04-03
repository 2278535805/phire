use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[repr(u8)]
#[serde(rename_all = "lowercase")]
pub enum ChartFormat {
    Rpe = 0,
    Pec = 1,
    Pgr = 2,
    Pbc = 3,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct ChartInfo {
    pub id: Option<i32>,
    pub uploader: Option<i32>,

    pub name: String,
    pub difficulty: f32,
    pub level: String,
    pub charter: String,
    pub composer: String,
    pub illustrator: String,

    pub chart: String,
    pub format: Option<ChartFormat>,
    pub music: String,
    pub illustration: String,

    pub preview_start: f32,
    pub preview_end: Option<f32>,
    pub aspect_ratio: f32,
    pub force_aspect_ratio: bool,
    pub background_dim: f32,
    pub line_reference_y_axis: bool,
    pub line_length: Option<f32>,
    pub offset: f32,
    pub tip: Option<String>,
    pub tags: Vec<String>,

    pub intro: String,

    pub hold_partial_cover: bool,
    pub note_uniform_scale: bool,
    pub score_total: u32,
    pub hold_particle_interval_ratio: f32,
    pub fold_animation: bool,

    pub created: Option<DateTime<Utc>>,
    pub updated: Option<DateTime<Utc>>,
    pub chart_updated: Option<DateTime<Utc>>,
}

impl Default for ChartInfo {
    fn default() -> Self {
        Self {
            id: None,
            uploader: None,

            name: "UK".to_string(),
            difficulty: 1.,
            level: "UK  Lv.1".to_string(),
            charter: "UK".to_string(),
            composer: "UK".to_string(),
            illustrator: "UK".to_string(),

            chart: "chart.json".to_string(),
            format: None,
            music: "song.mp3".to_string(),
            illustration: "background.png".to_string(),

            preview_start: 0.,
            preview_end: None,
            aspect_ratio: 16. / 9.,
            force_aspect_ratio: false,
            background_dim: 0.1,
            line_reference_y_axis: false,
            line_length: None,
            offset: 0.,
            tip: None,
            tags: Vec::new(),

            intro: String::new(),

            hold_partial_cover: false,
            note_uniform_scale: false,
            score_total: 1_000_000,
            hold_particle_interval_ratio: 1.0,
            fold_animation: false,

            created: None,
            updated: None,
            chart_updated: None,
        }
    }
}

impl ChartInfo {
    pub fn line_length(&self) -> f32 {
        if let Some(line_length) = self.line_length {
            line_length
        } else if self.line_reference_y_axis {
            5.75
        } else {
            6.0
        }
    }
}
