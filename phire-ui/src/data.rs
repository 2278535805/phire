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
            composer: String::new(),
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
        Ok(())
    }

    pub fn find_chart_by_path(&self, local_path: &str) -> Option<usize> {
        self.charts.iter().position(|local| local.local_path == local_path)
    }
}
