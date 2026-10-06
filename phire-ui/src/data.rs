use crate::{
    client::{Chart, Ptr, User},
    dir,
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use phire::{
    config::{Config, Mods},
    info::ChartInfo,
    scene::SimpleRecord,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, ops::DerefMut, path::Path};

fn default_score_total() -> u32 {
    1_000_000
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BriefChartInfo {
    pub id: Option<i32>,
    pub guid: Option<String>,
    pub uploader: Option<Ptr<User>>,
    pub name: String,
    pub level: String,
    pub difficulty: f32,
    #[serde(alias = "description")]
    pub intro: String,
    pub charter: String,
    pub composer: String,
    pub illustrator: String,
    #[serde(default="default_score_total")]
    pub score_total: u32,
    pub created: Option<DateTime<Utc>>,
    pub updated: Option<DateTime<Utc>>,
    pub chart_updated: Option<DateTime<Utc>>,
    #[serde(default)]
    pub has_unlock: bool,
}

impl BriefChartInfo {
    pub fn from_chart(chart: &Chart) -> Self {
        let name = chart
            .title
            .clone()
            .or_else(|| chart.song.as_ref().map(|s| s.title.clone()))
            .unwrap_or_default();
        Self {
            id: None,
            guid: Some(chart.id.clone()),
            uploader: Some(Ptr::new(chart.owner_id.to_string())),
            name,
            level: chart.level.clone(),
            difficulty: chart.difficulty,
            intro: chart.description.clone().unwrap_or_default(),
            charter: crate::client::parse_author_name(&chart.author_name),
            composer: chart.song.as_ref().and_then(|s| s.author_name.clone()).unwrap_or_default(),
            illustrator: chart.illustrator.clone().unwrap_or_default(),
            score_total: 1_000_000,
            created: chart.date_created,
            updated: chart.date_updated,
            chart_updated: chart.date_file_updated,
            has_unlock: false,
        }
    }
}

impl From<ChartInfo> for BriefChartInfo {
    fn from(info: ChartInfo) -> Self {
        Self {
            id: info.id,
            guid: info.guid,
            uploader: info.uploader.map(|id| Ptr::new(id.to_string())),
            name: info.name,
            level: info.level,
            difficulty: info.difficulty,
            intro: info.intro,
            charter: info.charter,
            composer: info.composer,
            illustrator: info.illustrator,
            score_total: info.score_total,
            created: info.created,
            updated: info.updated,
            chart_updated: info.chart_updated,
            has_unlock: info.unlock_video.is_some(),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct LocalChart {
    #[serde(flatten)]
    pub info: BriefChartInfo,
    pub local_path: String,
    pub record: Option<SimpleRecord>,
    #[serde(default)]
    pub mods: Mods,
    #[serde(default)]
    pub played_unlock: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayConfig {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    pub perfect_judgment: f64,
    pub good_judgment: f64,
    pub bad_judgment: f64,
}

impl Default for PlayConfig {
    fn default() -> Self {
        Self {
            id: None,
            name: "default".to_owned(),
            perfect_judgment: 0.08,
            good_judgment: 0.16,
            bad_judgment: 0.22,
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Data {
    pub me: Option<User>,
    pub charts: Vec<LocalChart>,
    pub config: Config,
    pub message_check_time: Option<DateTime<Utc>>,
    pub language: Option<String>,
    pub theme: usize,
    pub tokens: Option<(String, String)>,
    pub respacks: Vec<String>,
    pub respack_id: usize,
    pub accept_invalid_cert: bool,
    pub character_id: String,
    pub character_form_id: String,
    pub erosion_enabled: bool,
    pub revealed_forms: HashSet<(String, String)>,
    pub play_configs: Vec<PlayConfig>,
    pub active_play_config: Option<usize>,
}

impl Data {
    pub async fn init(&mut self) -> Result<()> {
        let charts = dir::charts()?;
        self.charts.retain(|it| Path::new(&format!("{}/{}", charts, it.local_path)).exists());
        let occurred: HashSet<_> = self.charts.iter().map(|it| it.local_path.clone()).collect();
        for entry in std::fs::read_dir(dir::custom_charts()?)? {
            let entry = entry?;
            let filename = entry.file_name();
            let filename = filename.to_str().unwrap();
            let filename = format!("custom/{filename}");
            if occurred.contains(&filename) {
                continue;
            }
            let path = entry.path();
            let Ok(mut fs) = phire::fs::fs_from_file(&path) else {
                continue;
            };
            let result = phire::fs::load_info(fs.deref_mut()).await;
            if let Ok(info) = result {
                self.charts.push(LocalChart {
                    info: BriefChartInfo { id: None, ..info.into() },
                    local_path: filename,
                    record: None,
                    mods: Mods::default(),
                    played_unlock: false,
                });
            }
        }
        for entry in std::fs::read_dir(dir::downloaded_charts()?)? {
            let entry = entry?;
            let filename = entry.file_name();
            let filename = filename.to_str().unwrap();
            let filename = format!("download/{filename}");
            let Ok(id): Result<i32, _> = filename.parse() else { continue };
            if occurred.contains(&filename) {
                continue;
            }
            let path = entry.path();
            let Ok(mut fs) = phire::fs::fs_from_file(&path) else {
                continue;
            };
            let result = phire::fs::load_info(fs.deref_mut()).await;
            if let Ok(info) = result {
                self.charts.push(LocalChart {
                    info: BriefChartInfo { id: Some(id), ..info.into() },
                    local_path: filename,
                    record: None,
                    mods: Mods::default(),
                    played_unlock: false,
                });
            }
        }
        if let Some(res_pack_path) = &mut self.config.res_pack_path {
            if res_pack_path.starts_with('/') {
                // for compatibility
                *res_pack_path = "chart.zip".to_owned();
            }
        }
        self.config.init();
        if self.play_configs.is_empty() {
            self.play_configs.push(PlayConfig::default());
        }
        if let Some(index) = self.active_play_config {
            if index >= self.play_configs.len() {
                self.active_play_config = Some(0);
            }
        } else {
            self.active_play_config = Some(0);
        }
        if let Some(pc) = self.active_play_config() {
            let (perfect, good, bad) = (pc.perfect_judgment, pc.good_judgment, pc.bad_judgment);
            self.config.perfect_judgment = perfect;
            self.config.good_judgment = good;
            self.config.bad_judgment = bad;
        }
        Ok(())
    }

    pub fn active_play_config(&self) -> Option<&PlayConfig> {
        self.active_play_config.and_then(|i| self.play_configs.get(i))
    }

    pub fn active_play_config_mut(&mut self) -> Option<&mut PlayConfig> {
        self.active_play_config.and_then(|i| self.play_configs.get_mut(i))
    }

    pub fn find_chart_by_path(&self, local_path: &str) -> Option<usize> {
        self.charts.iter().position(|local| local.local_path == local_path)
    }
}
