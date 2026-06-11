phire::tl_file!("character");

use std::collections::HashMap;

use macroquad::texture::load_texture;
use phire::{ext::SafeTexture, health::HealthConfig};
use serde::{Deserialize, Serialize};
use anyhow::Result;
use tracing::error;

use crate::get_data;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Character {
    pub id: String,
    pub name: HashMap<String, String>,
    pub intro: HashMap<String, String>,
    pub skill: HashMap<String, String>,
    pub illust: String,
    pub illustrator: String,

    #[serde(default)]
    pub name_size: Option<f32>,
    #[serde(default)]
    pub baseline: bool,

    pub position: (f32, f32, f32, f32),

    pub health_mode: Option<HealthConfig>,

    #[serde(skip)]
    pub illu: Option<SafeTexture>,
}

impl Character {
    pub fn name(&self) -> &str {
        let lang = get_data().language.as_deref().unwrap_or("en-US");
        self.name.get(lang)
            .or_else(|| self.name.get("en-US"))
            .map(|s| s.as_str())
            .unwrap_or("")
    }

    pub fn intro(&self) -> &str {
        let lang = get_data().language.as_deref().unwrap_or("en-US");
        self.intro.get(lang)
            .or_else(|| self.intro.get("en-US"))
            .map(|s| s.as_str())
            .unwrap_or("")
    }

    pub fn skill(&self) -> &str {
        let lang = get_data().language.as_deref().unwrap_or("en-US");
        self.skill.get(lang)
            .or_else(|| self.skill.get("en-US"))
            .map(|s| s.as_str())
            .unwrap_or("")
    }

    pub async fn load_by_id(id: &str) -> Result<Self> {
        let data = Self::load_all().await?;
        let character = data.iter().find(|c| c.id == id).map_or_else(|| &data[0], |c| c);
        Self::new(character.clone()).await
    }

    pub async fn load_all() -> Result<Vec<Self>> {
        let list = macroquad::file::load_string("char.json").await?;
        let list: Vec<String> = serde_json::from_str(&list)?;
        let mut data = Vec::new();
        for ch in list {
            let char = macroquad::file::load_string(&ch).await?;
            let char: Character = serde_json::from_str(&char)?;
            data.push(char);
        }
        Ok(data)
    }

    pub async fn new_all() -> Result<Vec<Self>> {
        let data = Self::load_all().await?;
        let mut result = Vec::new();
        for ch in data {
            result.push(Self::new(ch).await?);
        }
        Ok(result)
    }

    async fn new(data: Character) -> Result<Self> {
        let illu = if let Ok(illu) = load_texture(&data.illust).await {
            let illu: SafeTexture = illu.into();
            Some(illu.with_mipmap())
        } else {
            error!("failed to load character illustration {}", data.illust);
            None
        };
        Ok(Self {
            id: data.id,
            name: data.name,
            intro: data.intro,
            skill: data.skill,
            illust: data.illust,
            illustrator: data.illustrator,

            name_size: data.name_size,
            baseline: data.baseline,

            position: data.position,
            health_mode: data.health_mode,
            illu,
        })
    }
}