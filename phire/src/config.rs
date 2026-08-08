use std::fmt;

use bitflags::bitflags;
use serde::{Deserialize, Serialize};

use crate::health::HealthConfig;

bitflags! {
    #[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq, Debug)]
    #[serde(transparent)]
    pub struct Mods: i32 {
        const AUTOPLAY = 1;
        const FLIP_X = 2;
        const FADE_OUT = 4;
        const FULL_SCREEN_JUDGE = 8;
    }
}

#[derive(Clone, Deserialize, Serialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub enum ChallengeModeColor {
    White,
    Green,
    Blue,
    Red,
    Golden,
    #[default]
    Rainbow,
}

impl fmt::Display for ChallengeModeColor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            ChallengeModeColor::White => "white",
            ChallengeModeColor::Green => "green",
            ChallengeModeColor::Blue => "blue",
            ChallengeModeColor::Red => "red",
            ChallengeModeColor::Golden => "golden",
            ChallengeModeColor::Rainbow => "rainbow",
        };
        write!(f, "{}", s)
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(rename = "adjust_time_new")]
    pub auto_tweak_offset: bool,
    pub aggressive_chart: bool,
    pub aggressive_note: bool,
    pub aggressive_particle: bool,
    pub aspect_ratio: Option<f32>,
    pub audio_buffer_size: Option<u32>,
    #[cfg(target_os = "android")]
    pub audio_compatibility: bool,
    pub challenge_color: ChallengeModeColor,
    pub challenge_rank: u32,
    pub chart_debug_line: f32,
    pub chart_debug_note: f32,
    pub chart_ratio: f32,
    pub all_good: bool,
    pub all_bad: bool,
    pub double_click_to_pause: bool,
    pub fxaa: bool,
    pub interactive: bool,
    pub note_scale: f32,
    pub mods: Mods,
    pub mp_enabled: bool,
    pub mp_address: String,
    pub offline_mode: bool,
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    pub fullscreen_mode: bool,
    pub offset: f64,
    pub particle: bool,
    pub player_name: String,
    pub player_rks: f32,
    pub res_pack_path: Option<String>,
    pub sample_count: u32,
    pub low_resolution_mode: bool,
    pub show_acc: bool,
    pub speed: f32,
    pub touch_debug: bool,
    pub volume_music: f32,
    pub volume_sfx: f32,
    pub volume_bgm: f32,
    pub watermark: String,
    pub roman: bool,
    pub chinese: bool,
    pub combo: String,
    pub difficulty: String,
    pub enter_animation: bool,

    // for compatibility
    pub autoplay: Option<bool>,

    pub judge_offset: f64,

    pub render_line: bool,
    pub render_line_extra: bool,
    pub render_note: bool,
    pub render_double_hint: bool,
    pub render_ui_pause: bool,
    pub render_ui_name: bool,
    pub render_ui_level: bool,
    pub render_ui_score: bool,
    pub render_ui_combo: bool,
    pub render_ui_bar: bool,
    pub render_bg: bool,
    pub render_bg_dim: bool,
    pub preserve_framebuffer: bool,
    pub render_extra: bool,
    pub bg_blurriness: f32,

    pub max_particles: usize,

    pub fade: f32,
    pub alpha_tint: bool, // note.alpha <=0.5 blue, note.alpha >0.5 red

    pub rotation_mode: bool,

    pub play_start_time: f64,
    pub play_end_time: Option<f64>,
    #[cfg(feature = "play")]
    pub shake_play_mode: bool,
    #[cfg(feature = "play")]
    pub health_mode: Option<HealthConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            #[cfg(not(feature = "play"))]
            auto_tweak_offset: false,
            #[cfg(feature = "play")]
            auto_tweak_offset: true,
            #[cfg(not(feature = "play"))]
            aggressive_chart: false,
            #[cfg(feature = "play")]
            aggressive_chart: true,
            aggressive_note: false,
            aggressive_particle: false,
            aspect_ratio: None,
            audio_buffer_size: None,
            #[cfg(target_os = "android")]
            audio_compatibility: false,
            challenge_color: ChallengeModeColor::Rainbow,
            challenge_rank: 3,
            chart_debug_line: 0.0,
            chart_debug_note: 0.0,
            chart_ratio: 1.0,
            all_good: false,
            all_bad: false,
            double_click_to_pause: true,
            fxaa: false,
            interactive: true,
            mods: Mods::default(),
            mp_address: "mp2.phira.cn:12345".to_owned(),
            mp_enabled: false,
            note_scale: 1.0,
            offline_mode: false,
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            fullscreen_mode: false,
            offset: 0.0,
            particle: true,
            player_name: "Guest".to_string(),
            player_rks: 15.,
            res_pack_path: None,
            sample_count: 1,
            low_resolution_mode: false,
            show_acc: false,
            speed: 1.0,
            touch_debug: false,
            volume_music: 1.0,
            volume_sfx: 0.0,
            volume_bgm: 1.0,
            watermark: "".to_string(),
            roman: false,
            chinese: false,
            combo: "RECALL".to_string(),
            difficulty: "".to_string(),
            enter_animation: true,

            autoplay: None,

            judge_offset: 0.,

            render_line: true,
            render_line_extra: true,
            render_note: true,
            render_double_hint: true,
            render_ui_pause: true,
            render_ui_name: true,
            render_ui_level: true,
            render_ui_score: true,
            render_ui_combo: true,
            render_ui_bar: true,
            render_bg: true,
            render_bg_dim: true,
            preserve_framebuffer: false,
            render_extra: true,
            bg_blurriness: 80.,

            max_particles: 5000,

            fade: 0.,
            alpha_tint: false,

            rotation_mode: false,

            play_start_time: 0.,
            play_end_time: None,
            #[cfg(feature = "play")]
            shake_play_mode: false,
            #[cfg(feature = "play")]
            health_mode: None,
        }
    }
}

impl Config {
    pub fn init(&mut self) {
        if let Some(flag) = self.autoplay {
            self.mods.set(Mods::AUTOPLAY, flag);
        }
    }

    #[inline]
    pub fn has_mod(&self, m: Mods) -> bool {
        self.mods.contains(m)
    }

    #[inline]
    pub fn autoplay(&self) -> bool {
        self.has_mod(Mods::AUTOPLAY)
    }

    #[inline]
    pub fn flip_x(&self) -> bool {
        self.has_mod(Mods::FLIP_X)
    }

    #[inline]
    pub fn full_scrrn_judge(&self) -> bool {
        self.has_mod(Mods::FULL_SCREEN_JUDGE)
    }
}
