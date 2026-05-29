use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::{judge::Judgement};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct ComboHealConfig {
    pub combo_for_heal: usize,
}

impl Default for ComboHealConfig {
    fn default() -> Self {
        Self {
            combo_for_heal: 10,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[derive(Default)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct SpeedBasedJudgeConfig {
    pub success_factor: f32,
    pub failure_factor: f32,
    pub punish_factor: f32,
    pub max_speed: f32,
    pub min_speed: f32,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct SpeedBasedConfig {
    pub with_judge: SpeedBasedJudgeConfig,
    pub without_judge: SpeedBasedJudgeConfig,
}

impl Default for SpeedBasedConfig {
    fn default() -> Self {
        Self {
            with_judge: SpeedBasedJudgeConfig {
                success_factor: 0.1,
                failure_factor: 0.2,
                punish_factor: 0.0,
                max_speed: 1.0,
                min_speed: -8.0,
            },
            without_judge: SpeedBasedJudgeConfig {
                success_factor: 0.1,
                failure_factor: 0.0,
                punish_factor: 0.1,
                max_speed: 1.0,
                min_speed: -1.2,
            },
        }
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HealthType {
    Classic{},
    ComboHeal(ComboHealConfig),
    SpeedBased(SpeedBasedConfig),
}

#[derive(Default, Deserialize, Serialize, Clone)]
pub struct HealthState {
    pub speed_with_judge: f32,
    pub speed_without_judge: f32,
    pub cumulative_combo: usize,
    pub track_failed: bool,
    pub now_health: f32,
    last_update_time: f32,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct HealthConfig {
    pub mode: HealthType,
    pub max_health: f32,
    pub initial_health: f32,

    pub perfect_heal: bool,
    pub good_heal: bool,
    pub bad_heal: bool,
    pub perfect_factor: f32,
    pub good_factor: f32,
    pub bad_factor: f32,
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            mode: HealthType::SpeedBased(SpeedBasedConfig::default()),
            max_health: 100.0,
            initial_health: 70.0,

            perfect_heal: true,
            good_heal: false,
            bad_heal: false,
            perfect_factor: 1.0,
            good_factor: -1.0,
            bad_factor: -3.0,
        }
    }
}

impl HealthConfig {
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self).map_err(anyhow::Error::from)
    }

    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).map_err(anyhow::Error::from)
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Health {
    pub config: HealthConfig,

    pub state: HealthState,
}

impl Health {
    pub fn new(config: HealthConfig) -> Self {
        let state = HealthState {
            now_health: config.initial_health,
            ..Default::default()
        };
        Self {
            config,

            state
        }
    }

    pub fn reset(&mut self) {
        self.state = HealthState {
            now_health: self.config.initial_health,
            last_update_time: 0.0,
            ..Default::default()
        };
    }

    fn heal(&mut self, factor: f32) {
        match &self.config.mode {
            HealthType::SpeedBased(config) => {
                self.state.speed_with_judge = (self.state.speed_with_judge + factor * config.with_judge.success_factor).clamp(config.with_judge.min_speed, config.with_judge.max_speed);
                self.state.speed_without_judge = (self.state.speed_without_judge + config.without_judge.success_factor).clamp(config.without_judge.min_speed, config.without_judge.max_speed);
            }
            HealthType::ComboHeal(config) => {
                self.state.cumulative_combo += 1;
                if self.state.cumulative_combo >= config.combo_for_heal {
                    self.state.now_health = (self.state.now_health + factor).clamp(0.0, self.config.max_health);
                    self.state.cumulative_combo = 0;
                }
            }
            HealthType::Classic{} => {}
        }
    }

    fn damage(&mut self, factor: f32) {
        match &self.config.mode {
            HealthType::SpeedBased(config) => {
                self.state.speed_with_judge = (self.state.speed_with_judge + factor * config.with_judge.failure_factor).clamp(config.with_judge.min_speed, config.with_judge.max_speed);
                self.state.speed_without_judge = (self.state.speed_without_judge + config.without_judge.failure_factor).clamp(config.without_judge.min_speed, config.without_judge.max_speed);
            }
            _ => {
                self.state.now_health = (self.state.now_health + factor).clamp(0.0, self.config.max_health);
            }
        }
    }

    pub fn on_judge(&mut self, judge: Judgement) {
        match judge {
            Judgement::Perfect => {
                if self.config.perfect_heal {
                    self.heal(self.config.perfect_factor);
                } else {
                    self.damage(self.config.perfect_factor);
                }
            }
            Judgement::Good => {
                if self.config.good_heal {
                    self.heal(self.config.good_factor);
                } else {
                    self.damage(self.config.good_factor);
                }
            }
            _ => {
                if self.config.bad_heal {
                    self.heal(self.config.bad_factor);
                } else {
                    self.damage(self.config.bad_factor);
                }
            }
        }
    }

    pub fn update(&mut self, now: f32) {
        if let HealthType::SpeedBased(config) = &self.config.mode {
            let dt = now - self.state.last_update_time;
            self.state.last_update_time = now;
            let delta_health = self.state.speed_with_judge * dt;
            let new_health = self.state.now_health + delta_health + self.state.speed_without_judge * dt;
            self.state.now_health = new_health.clamp(0.0, self.config.max_health);
            self.state.speed_with_judge = (self.state.speed_with_judge - dt * config.with_judge.punish_factor).clamp(config.with_judge.min_speed, config.with_judge.max_speed);
            self.state.speed_without_judge = (self.state.speed_without_judge - dt * config.without_judge.punish_factor).clamp(config.without_judge.min_speed, config.without_judge.max_speed);
            if self.state.now_health <= 0.0 {
                self.state.track_failed = true;
            } else if self.state.now_health >= self.config.max_health {
                self.state.speed_with_judge = self.state.speed_with_judge.min(0.);
                self.state.speed_without_judge = self.state.speed_without_judge.min(0.);
            }
        }
    }
}