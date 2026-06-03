phire::tl_file!("character");

use std::collections::HashMap;

use macroquad::texture::load_texture;
use phire::ext::SafeTexture;
use serde::{Deserialize, Serialize};
use anyhow::Result;

use crate::get_data;

#[derive(Clone, Serialize, Deserialize)]
pub struct Character {
    pub id: String,
    pub name: HashMap<String, String>,
    pub intro: HashMap<String, String>,
    pub illust: String,
    pub illustrator: String,

    #[serde(default)]
    pub name_size: Option<f32>,
    #[serde(default)]
    pub baseline: bool,

    pub illu_adjust: (f32, f32, f32, f32),

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

    pub async fn load_first() -> Result<Self> {
        let data = Self::load_all().await?;
        let first = data.first().ok_or_else(|| anyhow::anyhow!("No characters found"))?;
        Self::new(first).await
    }

    pub async fn load_by_id(id: &str) -> Result<Self> {
        let data = Self::load_all().await?;
        let character = data.iter().find(|c| c.id == id).ok_or_else(|| anyhow::anyhow!("Character with id '{}' not found", id))?;
        Self::new(character).await
    }

    pub async fn load_all() -> Result<Vec<Self>> {
        let data = macroquad::file::load_string("/char/char.json").await?;
        let data: Vec<Character> = serde_json::from_str(&data)?;
        Ok(data)
    }

    async fn new(data: &Character) -> Result<Self> {
        let illu: SafeTexture = load_texture(&data.illust).await?.into();
        Ok(Self {
            id: data.id.clone(),
            name: data.name.clone(),
            intro: data.intro.clone(),
            illust: data.illust.clone(),
            illustrator: data.illustrator.clone(),

            name_size: data.name_size,
            baseline: data.baseline,

            illu_adjust: data.illu_adjust,

            illu: Some(illu.with_mipmap()),
        })
    }
}