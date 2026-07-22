#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i64)]
#[allow(unused)]
pub enum Permission {
    UploadChart     = 0x00000001,
    SeeUnreviewed   = 0x00000002,
    DeleteUnstable  = 0x00000004,
    Review          = 0x00000008,
    SeeStableReq    = 0x00000010,
    StabilizeChart  = 0x00000020,
    EditTags        = 0x00000040,
    StabilizeJudge  = 0x00000080,
    DeleteStable    = 0x00000100,
    SeeAllEvents    = 0x00000200,
    BanUser         = 0x00000400,
    SetRanked       = 0x00000800,
    SetAllRole      = 0x00001000,
    SetReviewer     = 0x00002000,
    SetSupervisor   = 0x00004000,
    BanAvatar       = 0x00008000,
    ReviewPecJam    = 0x00010000,
}

use super::{File, Object};
use crate::client::Client;
use anyhow::Result;
use chrono::{DateTime, Utc};
use image::DynamicImage;
use macroquad::prelude::Color;
use once_cell::sync::Lazy;
use phire::{ext::SafeTexture, task::Task};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;
use tracing::warn;

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: i32,
    #[serde(alias = "name")]
    pub user_name: String,
    #[serde(deserialize_with = "deser_avatar")]
    pub avatar: Option<File>,
    #[serde(default)]
    pub badges: Vec<String>,
    pub language: String,
    #[serde(alias = "bio")]
    pub biography: Option<String>,
    #[serde(alias = "exp")]
    pub experience: i64,
    pub rks: f32,
    #[serde(alias = "roles")]
    pub role: String,

    #[serde(alias = "joined")]
    pub date_joined: DateTime<Utc>,
    #[serde(alias = "last_login")]
    pub date_last_logged_in: DateTime<Utc>,
}

fn deser_avatar<'de, D>(deserializer: D) -> Result<Option<File>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value: Option<serde_json::Value> = Option::deserialize(deserializer)?;
    Ok(value.and_then(|v| match v {
        serde_json::Value::String(s) if !s.is_empty() => Some(File { url: s }),
        serde_json::Value::Object(_) => serde_json::from_value::<File>(v).ok(),
        _ => None,
    }))
}

impl Object for User {
    const QUERY_PATH: &'static str = "users";

    fn id(&self) -> String {
        self.id.to_string()
    }
}
impl User {
    pub fn perms(&self) -> i64 {
        match self.role.as_str() {
            "Administrator" => i64::MAX,
            "Moderator" => 0x000003ff,
            "Qualified" => 0x00000001,
            _ => 0,
        }
    }

    pub fn has_perm(&self, perm: i64) -> bool {
        self.perms() & perm != 0
    }

    pub fn name_color(&self) -> Color {
        Color::from_hex(if self.badges.iter().any(|it| it == "admin") {
            0x673ab7
        } else if self.badges.iter().any(|it| it == "sponsor") {
            0xff7043
        } else {
            0xffffff
        })
    }

    pub fn name(&self) -> &str {
        &self.user_name
    }
}

type UserTask = Task<Result<Option<DynamicImage>>>;
type UserTaskMap = HashMap<i32, UserTask>;
type UserResult = (String, Color, Option<Option<SafeTexture>>);
type UserResultMap = HashMap<i32, UserResult>;

static TASKS: Lazy<Mutex<UserTaskMap>> = Lazy::new(Mutex::default);
static RESULTS: Lazy<Mutex<UserResultMap>> = Lazy::new(Mutex::default);

pub struct UserManager;

impl UserManager {
    pub fn clear_cache(id: i32) -> Result<()> {
        TASKS.blocking_lock().remove(&id);
        RESULTS.blocking_lock().remove(&id);
        Ok(())
    }

    pub fn request(id: i32) {
        let mut tasks = TASKS.blocking_lock();
        if tasks.contains_key(&id) {
            return;
        }
        tasks.insert(
            id,
            Task::new(async move {
                let id_str = id.to_string();
                let user: Arc<User> = Client::load(&id_str).await?;
                RESULTS.lock().await.insert(id, (user.user_name.clone(), user.name_color(), None));
                if let Some(avatar) = &user.avatar {
                    Ok(Some(image::load_from_memory(&avatar.fetch().await?)?))
                } else {
                    Ok(None)
                }
            }),
        );
    }

    pub fn name_and_color(id: i32) -> Option<(String, Color)> {
        let names = RESULTS.blocking_lock();
        if let Some((name, color, ..)) = names.get(&id) {
            Some((name.to_owned(), *color))
        } else {
            None
        }
    }

    pub fn get_avatar(id: i32) -> Option<Option<SafeTexture>> {
        let mut guard = TASKS.blocking_lock();
        if let Some(task) = guard.get_mut(&id) {
            if let Some(result) = task.take() {
                match result {
                    Err(err) => {
                        warn!("Failed to fetch user info: {err:?}");
                        guard.remove(&id);
                    }
                    Ok(image) => {
                        RESULTS.blocking_lock().get_mut(&id).unwrap().2 = Some(image.map(|it| SafeTexture::from(it).with_mipmap()));
                    }
                }
            }
        } else {
            drop(guard);
        }
        RESULTS.blocking_lock().get(&id).and_then(|it| it.2.clone())
    }

    pub fn opt_avatar(id: i32, tex: &SafeTexture) -> Result<Option<SafeTexture>, SafeTexture> {
        Self::get_avatar(id).map(|it| it.ok_or_else(|| tex.clone())).transpose()
    }
}
