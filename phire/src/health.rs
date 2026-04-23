use serde::{Deserialize, Serialize};

use crate::{judge::Judgement};

#[derive(Clone, Deserialize, Serialize)]
pub enum HealthType {
    None,
    ComboHeal {
        combo_for_heal: usize,
    },
    SpeedBased {
        success_factor: f32,
        failure_factor: f32,
        max_health_judge_speed: f32,
        min_health_judge_speed: f32,
        max_health_time_speed: f32,
        min_health_time_speed: f32,
    },
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
        match self.mode {
            HealthType::SpeedBased { success_factor, max_health_judge_speed: max_health_speed, min_health_judge_speed: min_health_speed, .. } => {
                self.state.health_judge_speed = (self.state.health_judge_speed + factor * success_factor).clamp(min_health_speed, max_health_speed);
                self.state.health_time_speed = (self.state.health_time_speed + 0.1).clamp(-1.0, 1.0);
            }
            HealthType::ComboHeal { combo_for_heal } => {
                self.state.cumulative_combo += 1;
                if self.state.cumulative_combo >= combo_for_heal {
                    self.now_health = (self.now_health + factor).min(self.max_health);
                    self.state.cumulative_combo = 0;
                }
            }
            HealthType::None => {}
        }
    }

    pub fn damage(&mut self, factor: f32) {
        match self.mode {
            HealthType::SpeedBased { failure_factor, max_health_judge_speed: max_health_speed, min_health_judge_speed: min_health_speed, .. } => {
                self.state.health_judge_speed = (self.state.health_judge_speed - factor * failure_factor).clamp(min_health_speed, max_health_speed);
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
        if let HealthType::SpeedBased { max_health_time_speed, min_health_time_speed, .. } = self.mode {
            let dt = now - self.state.last_update_time;
            self.state.last_update_time = now;
            let delta_health = self.state.health_judge_speed * dt;
            let new_health = self.now_health + delta_health + self.state.health_time_speed * dt;
            self.now_health = new_health.clamp(0.0, self.max_health);
            self.state.health_time_speed = (self.state.health_time_speed - dt * 0.1).clamp(min_health_time_speed, max_health_time_speed);
            if self.now_health <= 0.0 {
                self.state.track_failed = true;
            }
        }
    }
}