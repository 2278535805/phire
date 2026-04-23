use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::{judge::Judgement};

fn default_combo_for_heal() -> usize { 10 }
fn default_success_factor() -> f32 { 0.1 }
fn default_failure_factor() -> f32 { 0.2 }
fn default_max_health_judge_speed() -> f32 { 1.0 }
fn default_min_health_judge_speed() -> f32 { -8.0 }
fn default_max_health_time_speed() -> f32 { 1.0 }
fn default_min_health_time_speed() -> f32 { -1.2 }

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComboHealConfig {
    #[serde(default = "default_combo_for_heal")]
    pub combo_for_heal: usize,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeedBasedConfig {
    #[serde(default = "default_success_factor")]
    pub success_factor: f32,
    #[serde(default = "default_failure_factor")]
    pub failure_factor: f32,
    #[serde(default = "default_max_health_judge_speed")]
    pub max_health_judge_speed: f32,
    #[serde(default = "default_min_health_judge_speed")]
    pub min_health_judge_speed: f32,
    #[serde(default = "default_max_health_time_speed")]
    pub max_health_time_speed: f32,
    #[serde(default = "default_min_health_time_speed")]
    pub min_health_time_speed: f32,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HealthType {
    Classic,
    ComboHeal(ComboHealConfig),
    SpeedBased(SpeedBasedConfig),
}

impl HealthType {
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self).map_err(|e| anyhow::Error::from(e))
    }

    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).map_err(|e| anyhow::Error::from(e))
    }
}

#[derive(Default, Deserialize, Serialize, Clone)]
pub struct HealthState {
    pub health_judge_speed: f32,
    pub health_time_speed: f32,
    pub cumulative_combo: usize,
    pub track_failed: bool,
    last_update_time: f32,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Health {
    pub mode: HealthType,
    pub max_health: f32,
    pub now_health: f32,

    pub state: HealthState,
}

impl Health {
    pub fn new(mode: HealthType, max_health: f32, initial_health: f32) -> Self {
        Self {
            mode,
            max_health,
            now_health: initial_health,

            state: HealthState::default()
        }
    }

    pub fn heal(&mut self, factor: f32) {
        match &self.mode {
            HealthType::SpeedBased(config) => {
                self.state.health_judge_speed = (self.state.health_judge_speed + factor * config.success_factor).clamp(config.min_health_judge_speed, config.max_health_judge_speed);
                self.state.health_time_speed = (self.state.health_time_speed + 0.1).clamp(-1.0, 1.0);
            }
            HealthType::ComboHeal(config) => {
                self.state.cumulative_combo += 1;
                if self.state.cumulative_combo >= config.combo_for_heal {
                    self.now_health = (self.now_health + factor).min(self.max_health);
                    self.state.cumulative_combo = 0;
                }
            }
            HealthType::Classic => {}
        }
    }

    pub fn damage(&mut self, factor: f32) {
        match &self.mode {
            HealthType::SpeedBased(config) => {
                self.state.health_judge_speed = (self.state.health_judge_speed - factor * config.failure_factor).clamp(config.min_health_judge_speed, config.max_health_judge_speed);
            }
            _ => {
                self.now_health = (self.now_health - factor).max(0.0);
                if self.now_health <= 0.0 {
                    self.state.track_failed = true;
                }
            }
        }
    }

    pub fn on_judge(&mut self, judge: Judgement) {
        match judge {
            Judgement::Perfect => {
                self.heal(1.0);
            }
            Judgement::Good => {
                self.damage(1.0);
            }
            _ => {
                self.damage(3.0);
            }
        }
    }

    pub fn update(&mut self, now: f32) {
        if let HealthType::SpeedBased(config) = &self.mode {
            let dt = now - self.state.last_update_time;
            self.state.last_update_time = now;
            let delta_health = self.state.health_judge_speed * dt;
            let new_health = self.now_health + delta_health + self.state.health_time_speed * dt;
            self.now_health = new_health.clamp(0.0, self.max_health);
            self.state.health_time_speed = (self.state.health_time_speed - dt * 0.1).clamp(config.min_health_time_speed, config.max_health_time_speed);
            if self.now_health <= 0.0 {
                self.state.track_failed = true;
            } else if self.now_health >= self.max_health {
                self.state.health_judge_speed = self.state.health_judge_speed.min(0.);
                self.state.health_time_speed = self.state.health_time_speed.min(0.);
            }
        }
    }
}