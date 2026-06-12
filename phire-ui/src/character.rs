phire::tl_file!("character");

use std::collections::HashMap;

use macroquad::texture::load_texture;
use phire::{ext::SafeTexture, health::HealthConfig};
use serde::{Deserialize, Serialize};
use anyhow::Result;
use tracing::error;

use crate::get_data;

fn default_visible() -> bool { true }

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CharacterForm {
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
    

    #[serde(default = "default_visible")]
    pub visible: bool,

    #[serde(skip)]
    pub illu: Option<SafeTexture>,
}

impl CharacterForm {
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
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Character {
    pub id: String,
    pub forms: Vec<CharacterForm>,

    #[serde(default)]
    pub list_name: HashMap<String, String>,

    #[serde(default = "default_visible")]
    pub visible: bool,

    #[serde(skip)]
    pub selected_form: usize,
}

impl Character {
    pub fn current_form(&self) -> &CharacterForm {
        &self.forms[self.selected_form.min(self.forms.len().saturating_sub(1))]
    }

    pub fn name(&self) -> &str {
        self.current_form().name()
    }

    pub fn list_name(&self) -> &str {
        let lang = get_data().language.as_deref().unwrap_or("en-US");
        self.list_name.get(lang)
            .or_else(|| self.list_name.get("en-US"))
            .map(|s| s.as_str())
            .unwrap_or_else(|| self.name())
    }

    pub fn intro(&self) -> &str {
        self.current_form().intro()
    }

    pub fn skill(&self) -> &str {
        self.current_form().skill()
    }

    pub fn set_form(&mut self, form_id: &str) {
        if let Some(pos) = self.forms.iter().position(|f| f.id == form_id) {
            self.selected_form = pos;
        }
    }

    pub fn form_count(&self) -> usize {
        self.visible_forms().count()
    }

    pub fn visible_forms(&self) -> impl Iterator<Item = &CharacterForm> {
        self.forms.iter().filter(|f| f.visible)
    }

    pub fn visible_forms_indices(&self) -> Vec<usize> {
        self.forms.iter().enumerate().filter(|(_, f)| f.visible).map(|(i, _)| i).collect()
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
        let mut forms = Vec::new();
        for form in data.forms {
            let illu = if let Ok(illu) = load_texture(&form.illust).await {
                let illu: SafeTexture = illu.into();
                Some(illu.with_mipmap())
            } else {
                error!("failed to load character illustration {}", form.illust);
                None
            };
            forms.push(CharacterForm {
                id: form.id,
                name: form.name,
                intro: form.intro,
                skill: form.skill,
                illust: form.illust,
                illustrator: form.illustrator,
                name_size: form.name_size,
                baseline: form.baseline,
                position: form.position,
                health_mode: form.health_mode,
                visible: form.visible,
                illu,
            });
        }
        Ok(Self {
            id: data.id,
            forms,
            list_name: data.list_name,
            visible: data.visible,
            selected_form: 0,
        })
    }
}
