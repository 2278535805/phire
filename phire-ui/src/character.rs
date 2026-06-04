phire::tl_file!("character");

use std::collections::HashMap;

use macroquad::texture::load_texture;
use phire::{ext::SafeTexture, health::HealthConfig};
use serde::{Deserialize, Serialize};
use anyhow::Result;

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
        Self::new(character).await
    }

    pub async fn load_all() -> Result<Vec<Self>> {
        let data = macroquad::file::load_string("/char/char.json").await?;
        let data: Vec<Character> = serde_json::from_str(&data)?;
        Ok(data)
    }

    pub async fn new_all() -> Result<Vec<Self>> {
        let data = Self::load_all().await?;
        let mut result = Vec::new();
        for ch in data {
            result.push(Self::new(&ch).await?);
        }
        Ok(result)
    }

    async fn new(data: &Character) -> Result<Self> {
        let illu: SafeTexture = load_texture(&data.illust).await?.into();
        Ok(Self {
            id: data.id.clone(),
            name: data.name.clone(),
            intro: data.intro.clone(),
            skill: data.skill.clone(),
            illust: data.illust.clone(),
            illustrator: data.illustrator.clone(),

            name_size: data.name_size,
            baseline: data.baseline,

            position: data.position,
            health_mode: data.health_mode.clone(),
            illu: Some(illu.with_mipmap()),
        })
    }
}