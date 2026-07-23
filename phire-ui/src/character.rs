phire::tl_file!("character");

use std::collections::HashMap;
use std::sync::Mutex;

use ::rand::rng;
use ::rand::Rng;
use macroquad::texture::load_texture;
use phire::{ext::SafeTexture, health::HealthConfig, judge::PlayResult, scene::LAST_RESULT};
use serde::{Deserialize, Serialize};
use anyhow::Result;
use tracing::error;

use crate::get_data;

pub static ALL_CHARACTERS: Mutex<Vec<Character>> = Mutex::new(Vec::new());
pub static CURRENT_CHARACTER: Mutex<Option<Character>> = Mutex::new(None);

fn default_visible() -> bool { true }

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErosionTarget {
    pub character: String,
    pub form: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErosionConfig {
    pub target: ErosionTarget,

    #[serde(default)]
    pub intro: HashMap<String, String>,

    #[serde(default)]
    pub track_complete: Option<bool>,
    #[serde(default)]
    pub full_combo: Option<bool>,
    #[serde(default)]
    pub min_score: Option<u32>,
    #[serde(default)]
    pub min_accuracy: Option<f32>,
    #[serde(default)]
    pub min_perfect: Option<u32>,
    #[serde(default)]
    pub max_miss: Option<u32>,
    #[serde(default)]
    pub max_late: Option<u32>,
    #[serde(default)]
    pub max_early: Option<u32>,
    #[serde(default)]
    pub probability: Option<f32>,

    #[serde(default)]
    pub force: bool,
}

impl ErosionConfig {
    pub fn intro(&self) -> &str {
        let lang = get_data().language.as_deref().unwrap_or("en-US");
        self.intro.get(lang)
            .or_else(|| self.intro.get("en-US"))
            .map(|s| s.as_str())
            .unwrap_or("")
    }

    pub fn has_intro(&self) -> bool {
        !self.intro.is_empty()
    }

    pub fn should_trigger(&self, result: &PlayResult) -> bool {
        let full_combo = result.max_combo == result.num_of_notes;

        if let Some(v) = self.track_complete { if result.track_complete != v { return false; } }
        if let Some(v) = self.full_combo { if full_combo != v { return false; } }
        if let Some(v) = self.min_score { if (result.score.round() as u32) < v { return false; } }
        if let Some(v) = self.min_accuracy { if (result.accuracy as f32) < v { return false; } }
        if let Some(v) = self.min_perfect { if result.counts[0] < v { return false; } }
        if let Some(v) = self.max_miss { if result.counts[3] > v { return false; } }
        if let Some(v) = self.max_late { if result.late > v { return false; } }
        if let Some(v) = self.max_early { if result.early > v { return false; } }
        if let Some(v) = self.probability { if rng().random::<f32>() >= v { return false; } }

        true
    }
}

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

    #[serde(default)]
    pub reveal: bool,

    pub erosion: Option<ErosionConfig>,

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

    pub fn erosion_target(&self) -> Option<&ErosionTarget> {
        self.current_form().erosion.as_ref().map(|e| &e.target)
    }

    pub fn form_count(&self) -> usize {
        self.visible_forms().count()
    }

    pub fn visible_forms(&self) -> impl Iterator<Item = &CharacterForm> {
        let revealed = &crate::get_data().revealed_forms;
        self.forms.iter().filter(move |f| {
            f.visible || (f.reveal && revealed.contains(&(self.id.clone(), f.id.clone())))
        })
    }

    pub fn visible_forms_indices(&self) -> Vec<usize> {
        let revealed = &crate::get_data().revealed_forms;
        self.forms.iter().enumerate()
            .filter(|(_, f)| {
                f.visible || (f.reveal && revealed.contains(&(self.id.clone(), f.id.clone())))
            })
            .map(|(i, _)| i)
            .collect()
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
                reveal: form.reveal,
                erosion: form.erosion,
                illu,
            });
        }
        Ok(Self {
            id: data.id,
            forms,
            list_name: data.list_name,
            selected_form: 0,
        })
    }
}

pub async fn init_characters() -> Result<()> {
    let mut all_characters = Character::new_all().await?;
    let idx = all_characters.iter().position(|c| c.id == crate::get_data().character_id).unwrap_or(0);
    all_characters[idx].set_form(&crate::get_data().character_form_id);
    let current = all_characters[idx].clone();
    *ALL_CHARACTERS.lock().unwrap() = all_characters;
    *CURRENT_CHARACTER.lock().unwrap() = Some(current);
    Ok(())
}

pub fn switch_to_erosion() {
    let (char_id, form_id) = {
        let character = CURRENT_CHARACTER.lock().unwrap();
        let character = character.as_ref().unwrap();
        let form = character.current_form();
        if let Some(e) = &form.erosion {
            if crate::get_data().erosion_enabled || e.force {
                (e.target.character.clone(), e.target.form.clone())
            } else {
                return;
            }
        } else {
            return;
        }
    };
    switch_character(&char_id, &form_id);
}

pub fn switch_character(character_id: &str, form_id: &str) {
    let mut all = ALL_CHARACTERS.lock().unwrap();
    if let Some(character) = all.iter_mut().find(|c| c.id == character_id) {
        character.set_form(form_id);
        *CURRENT_CHARACTER.lock().unwrap() = Some(character.clone());
        let data = crate::get_data_mut();
        data.character_id = character_id.to_owned();
        data.character_form_id = form_id.to_owned();
        let form = character.current_form();
        data.config.health_mode = form.health_mode.clone();
        if data.erosion_enabled || form.erosion.as_ref().map_or(false, |it| it.force) {
            data.revealed_forms.insert((character_id.to_owned(), form_id.to_owned()));
        }
        let _ = crate::save_data();
    }
}

pub fn check_erosion_trigger() {
    let Some(result) = LAST_RESULT.lock().ok().and_then(|mut r| r.take()) else {
        return;
    };
    let current = CURRENT_CHARACTER.lock().ok().and_then(|c| {
        let character = c.as_ref()?;
        let erosion = character.current_form().erosion.as_ref()?;
        if !crate::get_data().erosion_enabled && !erosion.force {
            return None;
        }
        Some(erosion.clone())
    });
    if let Some(erosion) = current {
        if erosion.should_trigger(&result) {
            switch_to_erosion();
        }
    }
}
