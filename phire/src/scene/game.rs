#![allow(unused)]

crate::tl_file!("game");

use chinese_number::{ChineseCase, ChineseCountMethod, ChineseVariant, NumberToChinese};
use super::{
    draw_background,
    ending::RecordUpdateState,
    loading::{BasicPlayer, UpdateFn, UploadFn},
    request_input, return_input, show_message, take_input, EndingScene, NextScene, Scene,
};
use crate::{
    bin::BinaryReader,
    config::{Config, Mods},
    core::{MAX_SIZE, MAX_SIZE_LIMIT, BUFFER_SIZE, BadNote, Chart, ChartExtra, Effect, Point, Resource, UIElement, HitSound},
    ext::{RectExt, SafeTexture, draw_text_aligned, draw_text_aligned_opt_width, ease_in_out_quartic, get_audio_latency, parse_time, push_frame_time, screen_aspect, semi_white, validate_combo},
    fs::FileSystem,
    gyro::GYRO,
    info::{ChartFormat, ChartInfo},
    judge::{Judge, ReplayData},
    parse::{RPE_WIDTH, parse_extra, parse_pec, parse_phigros, parse_rpe},
    task::Task,
    time::TimeManager,
    ui::{RectButton, Ui}
};
use anyhow::{bail, Context, Result};
use concat_string::concat_string;
use macroquad::{prelude::*, window::InternalGlContext};
use sasa::{Music, MusicParams, PlaySfxParams};
use serde::{Deserialize, Serialize};
use std::{
    io::Cursor,
    ops::{DerefMut, Range},
    sync::{Arc, Mutex},
};
use tracing::{debug, warn};

const PAUSE_CLICK_INTERVAL: f32 = 0.7;

// #[cfg(feature = "closed")]
// mod inner;
#[cfg(feature = "closed")]
use crate::inner::*;

pub const WAIT_TIME: f64 = 0.5;
const AFTER_TIME: f64 = 0.7;
const PAUSE_BACKGROUND_ALPHA: f32 = 0.6;
const TRAIL_DURATION: f64 = 0.6;
const TRAIL_RADIUS: f32 = 0.035;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimpleRecord {
    pub score: u32,
    pub accuracy: f32,
    pub full_combo: bool,
    #[serde(default)]
    pub track_complete: bool,
}

impl SimpleRecord {
    pub fn update(&mut self, other: &SimpleRecord) -> bool {
        let mut changed = false;
        if other.score > self.score {
            self.score = other.score;
            changed = true;
        }
        if other.accuracy > self.accuracy {
            self.accuracy = other.accuracy;
            changed = true;
        }
        if other.full_combo & !self.full_combo {
            self.full_combo = other.full_combo;
            changed = true;
        }
        if other.track_complete & !self.track_complete {
            self.track_complete = other.track_complete;
            changed = true;
        }
        changed
    }
}

fn fmt_time(t: f64) -> String {
    let f = t < 0.;
    let t = t.abs();
    let secs = t % 60.;
    let mut t = (t / 60.) as u64;
    let mins = t % 60;
    t /= 60;
    let hrs = t % 100;
    format!("{}{hrs:02}:{mins:02}:{secs:05.2}", if f { "-" } else { "" })
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    fn on_game_start();
}

#[derive(PartialEq, Eq)]
pub enum GameMode {
    Normal,
    TweakOffset,
    Exercise,
    NoRetry,
    View,
    Replay,
}

/// The replay data of the most recent qualifying play, consumed by the replay viewer.
pub static LAST_REPLAY: Mutex<Option<(String, ReplayData)>> = Mutex::new(None);

#[derive(Clone)]
enum State {
    Starting,
    BeforeMusic,
    Playing,
    Ending,
}

pub struct PauseRewind {
    time: Option<f64>,
    duration: Option<f64>,
    dim: bool,
}

pub struct GameScene {
    should_exit: bool,
    next_scene: Option<NextScene>,

    pub mode: GameMode,
    pub res: Resource,
    pub chart: Chart,
    pub judge: Judge,
    pub gl: InternalGlContext<'static>,
    player: Option<BasicPlayer>,
    info_offset: f64,
    effects: Vec<Effect>,

    first_in: bool,
    exercise_range: Range<f64>,
    exercise_press: Option<(i8, u64)>,
    exercise_btns: (RectButton, RectButton),

    pub music: Music,
    sfx_vec: Option<(Vec<f64>, Vec<f64>, Vec<f64>)>,


    state: State,
    pub last_update_time: f64,
    pub first_update_time: f64,
    pause_rewind: PauseRewind,
    pause_first_time: f32,

    pub bad_notes: Vec<BadNote>,

    upload_fn: Option<UploadFn>,
    refresh_task: Option<Task<()>>,
    update_fn: Option<UpdateFn>,

    pub touch_points: Vec<(f32, f32)>,

    pub replay_trails: Vec<(f64, Vec2)>,
}

macro_rules! reset {
    ($self:ident, $res:expr, $tm:ident) => {{
        $self.bad_notes.clear();
        $self.judge.reset();
        $self.chart.reset();
        $res.reset();
        $self.music.pause()?;
        $self.music.seek_to(0.)?;
        $tm.speed = $res.config.speed as _;
        $tm.reset();
        $self.last_update_time = $tm.now();
        $self.state = State::Starting;
        $self.pause_rewind = PauseRewind {
            time: None,
            duration: None,
            dim: false
        };
        if let Some((sfx_click_vec, sfx_drag_vec, sfx_flick_vec)) = &$self.sfx_vec {
            $res.sfx_click.schedule_play(sfx_click_vec, PlaySfxParams {
                amplifier: $res.config.volume_sfx,
            })?;
            $res.sfx_drag.schedule_play(sfx_drag_vec, PlaySfxParams {
                amplifier: $res.config.volume_sfx,
            })?;
            $res.sfx_flick.schedule_play(sfx_flick_vec, PlaySfxParams {
                amplifier: $res.config.volume_sfx,
            })?;
        }
    }};
}

macro_rules! reset_music_speed {
    ($self:ident, $res:expr, $tm:ident) => {{
        debug!("recreate music");
        $self.music = Self::new_music($res).expect("failed to create music");
        $tm.pause();
        $self.music.pause().ok();
        let now = $tm.now();
        $tm.speed = $res.config.speed as _;
        $tm.seek_to(now);
        $self.music.seek_to(now).ok();
    }};
}

fn round_to_step(v: f64, step: f64) -> f64 {
    (v / step).round() * step
}

fn parse_note_list(time_list: Vec<f64>, mix_opt: bool) -> Vec<f64> {
    let mut time_list = time_list;
    time_list.sort_by(|a, b| {
        let a = round_to_step(*a, 0.005);
        let b = round_to_step(*b, 0.005);
        a.total_cmp(&b)
    });

    if !mix_opt {
        return time_list;
    }

    let step = 1. / 1000.;

    let mut kept_sfx_list = Vec::with_capacity(time_list.len());
    let mut last_t = 0.0;
    let mut count = 0;

    for &pos in time_list.iter() {
        let round_pos = round_to_step(pos, step);
        let is_new_group = round_pos != last_t;

        if is_new_group {
            last_t = round_pos;
            count = 1;
            kept_sfx_list.push(round_pos);
        } else {
            if count < 3 {
                kept_sfx_list.push(round_pos);
                count += 1;
            }
        }
    }
    kept_sfx_list
}

fn parse_sfx_list(sfx_list: Vec<f64>, mix_opt: bool) -> Vec<f64> {
    let mut sfx_list = sfx_list;
    sfx_list.sort_by(|a, b| {
        let a = round_to_step(*a, 0.005);
        let b = round_to_step(*b, 0.005);
        a.total_cmp(&b)
    });

    let step = if mix_opt { 0.001 } else { 0.0005 };

    let mut kept_sfx_list = Vec::with_capacity(sfx_list.len());
    let mut last_t = 0.0;
    let mut count = 0;

    for &pos in sfx_list.iter() {
        let round_pos = round_to_step(pos, step);
        let is_new_group = round_pos != last_t;

        if is_new_group {
            last_t = round_pos;
            count = 1;
            if mix_opt {
                kept_sfx_list.push(round_pos);
            } else {
                kept_sfx_list.push(pos);
            };
        } else {
            if count < 3 {
                if mix_opt {
                    kept_sfx_list.push(round_pos);
                } else {
                    kept_sfx_list.push(pos);
                };
                count += 1;
            }
        }
    }
    kept_sfx_list
}

fn offset_sfx_list(sfx_list: &mut (Vec<f64>, Vec<f64>, Vec<f64>), res: &mut Resource, offset: f64) -> Result<()> {
    sfx_list.0.iter_mut().for_each(|t| *t += offset);
    sfx_list.1.iter_mut().for_each(|t| *t += offset);
    sfx_list.2.iter_mut().for_each(|t| *t += offset);
    res.sfx_click.schedule_play(&sfx_list.0, PlaySfxParams {
        amplifier: res.config.volume_sfx,
    })?;
    res.sfx_drag.schedule_play(&sfx_list.1, PlaySfxParams {
        amplifier: res.config.volume_sfx,
    })?;
    res.sfx_flick.schedule_play(&sfx_list.2, PlaySfxParams {
        amplifier: res.config.volume_sfx,
    })?;
    Ok(())
}

fn peak_density(times: &[f64]) -> usize {
    let mut max = 0;
    let mut r = 0;
    for l in 0..times.len() {
        let limit = times[l] + 0.5;
        while r < times.len() && times[r] < limit { r += 1; }
        if r - l > max { max = r - l; }
        if r == times.len() { break; }
    }
    max
}

impl GameScene {
    pub const BEFORE_TIME: f64 = 0.7;
    pub const BEFORE_DURATION: f64 = 1.2;
    pub const WAIT_AFTER_TIME: f64 = AFTER_TIME + 0.3;
    pub const FADEOUT_TIME: f64 = WAIT_TIME + Self::WAIT_AFTER_TIME;

    pub async fn load_chart_bytes(fs: &mut dyn FileSystem, info: &ChartInfo) -> Result<Vec<u8>> {
        if let Ok(bytes) = fs.load_file(&info.chart).await {
            return Ok(bytes);
        }
        if let Some(name) = info.chart.strip_suffix(".pec") {
            if let Ok(bytes) = fs.load_file(&concat_string!(name, ".json")).await {
                return Ok(bytes);
            }
        }
        bail!("Cannot find chart file")
    }

    pub fn int_to_roman(mut num: u32) -> String {
        if num.to_string() == "0" {
            return "-".to_string()
        };
        let mut roman: String = String::new();
        let roman_numerals = [
            (1000000, "M￣"),
            (900000, "CM￣"),
            (500000, "D￣"),
            (400000, "CD￣"),
            (100000, "C￣"),
            (90000, "XC￣"),
            (50000, "L￣"),
            (40000, "XL￣"),
            (10000, "X￣"),
            (1000, "M"),
            (900, "CM"),
            (500, "D"),
            (400, "CD"),
            (100, "C"),
            (90, "XC"),
            (50, "L"),
            (40, "XL"),
            (10, "X"),
            (9, "IX"),
            (5, "V"),
            (4, "IV"),
            (1, "I"),
        ];
    
        for &(value, symbol) in roman_numerals.iter() {
            while num >= value {
                roman.push_str(symbol);
                num -= value;
            }
        }
        roman
        
    }

    pub fn int_to_chinese(num: u32) -> String {
        num.to_chinese(ChineseVariant::Simple, ChineseCase::Lower, ChineseCountMethod::TenThousand).unwrap()
    }

    pub fn float_to_chinese(num: f32) -> String {
        let chinese_digits = ["零", "一", "二", "三", "四", "五", "六", "七", "八", "九"];
        let chinese_units = ["", "十", "百", "千", "万", "十万", "百万", "千万", "亿"];
    
        let integer_part = num.trunc() as u64;
        let decimal_part = (num.fract() * 100.0).round() / 100.0;
    
        let mut result = String::new();
    
        // 整数
        if integer_part == 0 {
            result.push_str(chinese_digits[0]);
        } else {
            let mut n = integer_part;
            let mut unit_index = 0;
            let mut need_zero = false;
    
            while n > 0 {
                let digit = (n % 10) as usize;
                if digit != 0 {
                    if need_zero {
                        result.insert(0, '零');
                        need_zero = false;
                    }
                    result.insert_str(0, chinese_units[unit_index]);
                    result.insert_str(0, chinese_digits[digit]);
                } else if !result.starts_with("零") {
                    need_zero = true;
                }
                n /= 10;
                unit_index += 1;
            }
    
            if result.starts_with("一十") {
                result.remove(0);
            }
            if result.ends_with("零") {
                result.pop();
            }
        }
    
        // 小数
        if decimal_part > 0.0 {
            result.push('点');
            let decimal_str = decimal_part.to_string();
            for c in decimal_str.chars().skip(2) { // 跳过"0."
                let digit = c.to_digit(10).unwrap() as usize;
                result.push_str(chinese_digits[digit]);
            }
        }
    
        result
    }
    

    pub async fn load_chart(fs: &mut dyn FileSystem, info: &ChartInfo, config: &Config) -> Result<(Chart, ChartFormat)> {
        let extra = if config.render_extra {
            if let Some(extra) = fs.load_file("extra.json").await.ok().map(String::from_utf8).transpose()? {
                parse_extra(&extra, fs).await.context("Failed to parse extra")?
            } else if let Some(extra) = fs.load_file("extra1.json").await.ok().map(String::from_utf8).transpose()? {
                parse_extra(&extra, fs).await.context("Failed to parse extra1")?
            } else {
                ChartExtra::default()
            }
        } else {
            ChartExtra::default()
        };
        let bytes = Self::load_chart_bytes(fs, info).await.context("Failed to load chart")?;
        let text = std::str::from_utf8(&bytes);
        let format = info.format.clone().unwrap_or_else(|| {
            if let Ok(text) = text {
                if text.starts_with('{') {
                    if text.contains("\"META\"") {
                        ChartFormat::Rpe
                    } else {
                        ChartFormat::Pgr
                    }
                } else {
                    ChartFormat::Pec
                }
            } else {
                ChartFormat::Pbc
            }
        });
        let mut chart = match format {
            ChartFormat::Rpe => parse_rpe(text?, fs, extra).await,
            ChartFormat::Pgr => parse_phigros(text?, extra),
            ChartFormat::Pec => parse_pec(text?, extra),
            ChartFormat::Pbc => {
                let mut r = BinaryReader::new(Cursor::new(bytes));
                r.read()
            }
        }?;
        chart.load_textures(fs).await?;
        Ok((chart, format))
    }

    pub async fn new(
        preload_chart: Option<(Chart, ChartFormat)>,
        mode: GameMode,
        info: ChartInfo,
        mut config: Config,
        mut fs: Box<dyn FileSystem>,
        player: Option<BasicPlayer>,
        background: SafeTexture,
        illustration: SafeTexture,
        upload_fn: Option<UploadFn>,
        update_fn: Option<UpdateFn>,
        replay: Option<ReplayData>,
    ) -> Result<Self> {
        if mode == GameMode::TweakOffset {
            config.mods.insert(Mods::AUTOPLAY);
            config.volume_music = config.volume_music.max(0.5);
            config.volume_sfx = config.volume_sfx.max(0.5);
        }
        let (mut chart, format) = if let Some((chart, format)) = preload_chart {
            (chart, format)
        } else {
            Self::load_chart(fs.deref_mut(), &info, &config).await?
        };
        let effects = std::mem::take(&mut chart.extra.global_effects);
        if config.fxaa {
            chart
                .extra
                .effects
                .push(Effect::new(0.0..f64::INFINITY, include_str!("fxaa.glsl"), Vec::new(), false).unwrap());
        }

        let mut judge = Judge::new(&chart);
        judge.set_limits(config.perfect_judgment, config.good_judgment, config.bad_judgment);
        if mode == GameMode::Replay {
            match replay {
                Some(data) => judge.load_replay(data),
                None => bail!("replay data not provided"),
            }
        } else if mode == GameMode::Normal && !config.autoplay() {
            judge.start_recording();
        }

        let info_offset = info.offset;
        let offset = chart.offset + info_offset;

        let (max_note, sfx_vec) = {
            let sfx = config.high_precision_sfx && config.autoplay() && config.volume_sfx >= 1e-2;
            let mut time_vec = Vec::with_capacity(chart.lines.iter().map(|line| line.notes.len()).sum());
            let mut sfx_click_vec = Vec::new();
            let mut sfx_drag_vec = Vec::new();
            let mut sfx_flick_vec = Vec::new();
            let t = if mode == GameMode::TweakOffset {
                chart.offset + info_offset
            } else {
                offset
            };
            chart.lines.iter().for_each(|line| line.notes.iter().for_each(|note| {
                time_vec.push(note.time + t);
                if note.fake { return; }
                if !sfx { return; }
                match note.hitsound {
                    HitSound::Click => sfx_click_vec.push(note.time + t),
                    HitSound::Drag => sfx_drag_vec.push(note.time + t),
                    HitSound::Flick => sfx_flick_vec.push(note.time + t),
                    _ => {},
                }
            }));
            time_vec = parse_note_list(time_vec, config.aggressive_note);
            let max_note = (peak_density(&time_vec) + 64).clamp(MAX_SIZE, MAX_SIZE_LIMIT);
            debug!("notes = {}, max_note = {}", time_vec.len(), max_note);
            drop(time_vec);
            if sfx {
                sfx_click_vec = parse_sfx_list(sfx_click_vec, mode != GameMode::TweakOffset);
                sfx_drag_vec = parse_sfx_list(sfx_drag_vec, mode != GameMode::TweakOffset);
                sfx_flick_vec = parse_sfx_list(sfx_flick_vec, mode != GameMode::TweakOffset);
                debug!(
                    "Prepared {} click, {} drag, {} flick sfx",
                    sfx_click_vec.len(), sfx_drag_vec.len(), sfx_flick_vec.len()
                );
            }
            (
                max_note,
                if sfx { Some((sfx_click_vec, sfx_drag_vec, sfx_flick_vec)) } else { None }
            )
        };

        let sfx_buffer_size = if let Some((sfx_click_vec, sfx_drag_vec, sfx_flick_vec)) = &sfx_vec {
            Some((
                (peak_density(sfx_click_vec) + 16).clamp(64, 3072),
                (peak_density(sfx_drag_vec) + 16).clamp(64, 3072),
                (peak_density(sfx_flick_vec) + 16).clamp(64, 3072),
            ))
        } else {
            None
        };

        let mut res = Resource::new(
            config,
            info,
            fs,
            player.as_ref().and_then(|it| it.avatar.clone()),
            background,
            illustration,
            chart.extra.effects.is_empty() && effects.is_empty(),
            max_note,
            sfx_buffer_size,
        )
        .await
        .context("Failed to load resources")?;

        if matches!(format, ChartFormat::Rpe) {
            res.info.line_length *= 4000. / RPE_WIDTH / 6.;
        }

        let exercise_range = offset + res.config.play_start_time..res.track_length;
        
        // Prepare extra sfx from chart.hitsounds
        chart.hitsounds.drain().for_each(|(name, clip)| {
            if let Ok(clip) = res.audio.create_sfx(clip, Some(BUFFER_SIZE)) {
                res.extra_sfxs.insert(name, clip);
            }
        });
        res.fonts = std::mem::take(&mut chart.fonts);

        let music = Self::new_music(&mut res)?;

        if let Some((sfx_click_vec, sfx_drag_vec, sfx_flick_vec)) = &sfx_vec {
            res.sfx_click.set_clock(music.clock())?;
            res.sfx_drag.set_clock(music.clock())?;
            res.sfx_flick.set_clock(music.clock())?;
            res.sfx_click.schedule_play(sfx_click_vec, PlaySfxParams {
                amplifier: res.config.volume_sfx,
            })?;
            res.sfx_drag.schedule_play(sfx_drag_vec, PlaySfxParams {
                amplifier: res.config.volume_sfx,
            })?;
            res.sfx_flick.schedule_play(sfx_flick_vec, PlaySfxParams {
                amplifier: res.config.volume_sfx,
            })?;
        };

        Ok(Self {
            should_exit: false,
            next_scene: None,

            mode,
            res,
            chart,
            judge,
            gl: unsafe { get_internal_gl() },
            player,
            effects,
            info_offset,

            first_in: false,
            exercise_range,
            exercise_press: None,
            exercise_btns: (RectButton::new(), RectButton::new()),

            music,
            sfx_vec,

            state: State::Starting,
            last_update_time: 0.,
            first_update_time: 0.,
            pause_rewind: PauseRewind {
                time: None,
                duration: None,
                dim: false
            },
            pause_first_time: f32::NEG_INFINITY,

            bad_notes: Vec::new(),

            upload_fn,
            refresh_task: None,
            update_fn,

            touch_points: Vec::new(),

            replay_trails: Vec::new(),
        })
    }

    fn new_music(res: &mut Resource) -> Result<Music> {
        let music = res.audio.create_music(
            res.music.clone(),
            MusicParams {
                amplifier: res.config.volume_music as _,
                playback_rate: res.config.speed as _,
                ..Default::default()
            },
        )?;
        res.sfx_click.set_clock(music.clock())?;
        res.sfx_drag.set_clock(music.clock())?;
        res.sfx_flick.set_clock(music.clock())?;
        Ok(music)
    }

    fn touch_scale(&self) -> f32 {
        (screen_width() / screen_height()) / self.res.aspect_ratio
    }

    fn ui(&mut self, ui: &mut Ui, tm: &mut TimeManager) -> Result<()> {
        let time = tm.now();
        let p = match self.state {
            State::Starting => {
                if time <= Self::BEFORE_TIME {
                    1. - (1. - time / Self::BEFORE_TIME).clamp(0., 1.).powi(3) as f32
                } else {
                    1.
                }
            }
            State::BeforeMusic => 1.,
            State::Playing => 1.,
            State::Ending => {
                let t = time - self.res.track_length - WAIT_TIME;
                1. - (t / (Self::WAIT_AFTER_TIME)).clamp(0., 1.).powi(2) as f32
            }
        };
        let c = Color::new(1., 1., 1., self.res.alpha);
        let res = &mut self.res;
        let aspect_ratio = res.aspect_ratio;
        let screen_aspect = screen_aspect();
        let scale_ratio = 1.777777;
        let top: f32 = -1.;
        let eps: f32 = 2e-2;
        let margin = 0.0425 * scale_ratio;
        let pause_w = 0.011 * scale_ratio;
        let pause_h = pause_w * 3.5;
        let pause_center = Point::new(-aspect_ratio + 0.0525 * scale_ratio, top + eps * 3.6454 - (1. - p) * 0.4 + pause_h / 2.);
        if res.config.interactive
            && !tm.paused()
            && self.pause_rewind.time.is_none()
            && matches!(self.state, State::Playing)
            && Judge::get_touches(res.config.chart_ratio, res.resolution_ratio).iter().any(|touch| {
                touch.phase == TouchPhase::Started && {
                    let p = touch.position;
                    let p = Point::new(p.x * screen_aspect, p.y * screen_aspect);
                    (pause_center - p).norm() < 0.05
                }
            })
        {
            let t = tm.now() as f32;
            if t - self.pause_first_time > PAUSE_CLICK_INTERVAL && res.config.double_click_to_pause {
                self.pause_first_time = t;
            } else {
                self.pause_first_time = f32::NEG_INFINITY;
                if !self.music.paused() {
                    self.music.fade_out(0.3)?;
                }
                tm.pause();
            }
        }
        if tm.now() as f32 - self.pause_first_time <= PAUSE_CLICK_INTERVAL {
            ui.fill_circle(pause_center.x, pause_center.y, 0.05 * scale_ratio, Color::new(1., 1., 1., 0.5));
        }

        let score = (self.judge.score() / 1_000_000. * res.info.score_total as f64).round() as u32;
        let score = if res.config.roman {
            Self::int_to_roman(score)
        } else if res.config.chinese {
            Self::int_to_chinese(score)
        }
        else {
            let width = res.info.score_total.to_string().len();
            format!("{:0>width$}", score, width = width)
        };
        let score_top = top + eps * 2.8125 - (1. - p) * 0.4;
        let score_right = aspect_ratio - margin + 0.001;
        let mut text_size = 0.71 * scale_ratio;
        let mut text = ui.text(&score).size(text_size);
        let max_width = 0.55 * aspect_ratio;
        let text_width = text.measure().w;
        if text_width > max_width {
            text_size *= max_width / text_width
        }
        self.chart.with_element(ui, res, UIElement::Score, Some((score_right, score_top)), Some((score_right, score_top)), |ui, color| {
            if res.config.render_ui_score {
                ui.text(score)
                    .pos(score_right, score_top)
                    .anchor(1., 0.)
                    .size(text_size)
                    .color(Color { a: color.a * c.a, ..color })
                    .draw();
            }
            if res.config.show_acc {
                ui.text(format!("{:05.2}%", self.judge.real_time_accuracy() * 100.))
                    .pos(aspect_ratio - margin, top + eps * 2.2 - (1. - p) * 0.4 + 0.07 + 0.05)
                    .anchor(1., 0.)
                    .size(0.4 * scale_ratio)
                    .color(Color { a: color.a * c.a * 0.7, ..color })
                    .draw();
            }
        });
        if res.config.render_ui_pause {
            self.chart.with_element(ui, res, UIElement::Pause, Some((pause_center.x - pause_w * 1.5, pause_center.y - pause_h * 0.5)), Some((pause_center.x - pause_w * 1.5, pause_center.y - pause_h * 0.5)), |ui, color| {
                let mut r = Rect::new(pause_center.x - pause_w / 2., pause_center.y - pause_h / 2., pause_w, pause_h);
                //let ct = pause_center.coords;
                let c = Color { a: color.a * c.a, ..color };
                
                r.x -= pause_w;
                ui.fill_rect(r, c);
                r.x += pause_w * 2.;
                ui.fill_rect(r, c);
            });
        }
        if self.judge.combo() >= 3 && res.config.render_ui_combo {
            let combo = if res.config.roman {
                Self::int_to_roman(self.judge.combo())
            } else if res.config.chinese {
                Self::int_to_chinese(self.judge.combo())
            }
            else {
                self.judge.combo().to_string()
            };
            let mut text_size = 0.98 * scale_ratio;
            let max_width = 0.55 * aspect_ratio;
            let text = ui.text(&combo).size(text_size).measure();
            let text_width = text.w;
            if text_width > max_width {
                text_size *= max_width / text_width
            }
            let combo_y = top + eps * 1.55 - (1. - p) * 0.4 + 0.055;
            let btm = self.chart.with_element(ui, res, UIElement::ComboNumber, Some((0., combo_y)), Some((0., combo_y)), |ui, color| {
                draw_text_aligned_opt_width(
                    ui,
                    &combo,
                    0., combo_y,
                    (0.5, 0.5),
                    text_size,
                    Color { a: color.a * c.a, ..color },
                    0.55 * aspect_ratio
                ).bottom() + 0.03 + 0.005
            });
            self.chart.with_element(ui, res, UIElement::Combo, Some((0., btm)), Some((0., btm)), |ui, color| {
                if (cfg!(feature = "play") && res.config.autoplay()) || validate_combo(&res.config.combo) || res.config.combo.len() > 50 {
                    draw_text_aligned(ui, "AUTOPLAY", 0., btm, (0.5, 0.5), 0.34 * scale_ratio, Color { a: color.a * c.a, ..color });
                    return;
                }
                draw_text_aligned_opt_width(ui, &res.config.combo, 0., btm, (0.5, 0.5), 0.34 * scale_ratio, Color { a: color.a * c.a, ..color }, 0.55 * aspect_ratio);
            });
        }
        let lf = -aspect_ratio + margin;
        let bt = -top - eps * 3.5 + (1. - p) * 0.4;
        #[cfg(feature = "play")]
        if res.config.health_mode.is_some() && matches!(self.mode, GameMode::Normal | GameMode::NoRetry | GameMode::View | GameMode::Replay) {
            let w = aspect_ratio * 0.05;
            let y = -top - eps * 9.;
            let h = top * 2. + eps * 23.;
            let dh = res.health.state.now_health / res.health.config.max_health * h;
            ui.fill_rect(
                Rect::new(lf, y, w, h),
                Color::new(0.4, 0.4, 0.4, c.a),
            );
            ui.fill_rect(
                Rect::new(lf, y, w, dh),
                Color::new(0.6, 0.6, 0.6, c.a),
            );
            draw_text_aligned_opt_width(ui, &format!("{:.0}", &res.health.state.now_health), lf + w * 0.5, y + dh - 0.01, (0.5, 1.), 0.4 * scale_ratio, semi_white(0.8 * c.a), 0.9 * aspect_ratio);
        }
        if res.config.render_ui_name {
            self.chart.with_element(ui, res, UIElement::Name, Some((lf, bt)), Some((lf, bt)), |ui, color| {
                draw_text_aligned_opt_width(ui, &res.info.name, lf, bt, (0., 1.), 0.505 * scale_ratio, Color { a: color.a * c.a, ..color }, 0.9 * aspect_ratio);
            });
        }
        if res.config.render_ui_level {
            self.chart.with_element(ui, res, UIElement::Level, Some((-lf, bt)), Some((-lf, bt)), |ui, color| {
                draw_text_aligned_opt_width(ui, &res.info.level, -lf, bt, (1., 1.), 0.505 * scale_ratio, Color { a: color.a * c.a, ..color }, 0.9 * aspect_ratio);
            });
        }
        if !res.config.watermark.is_empty() {
            draw_text_aligned_opt_width(ui, &res.config.watermark, 0., -top * 0.98 + (1. - p) * 0.4, (0.5, 1.), 0.25 * scale_ratio, semi_white(0.5 * c.a), 2.0 * aspect_ratio);
            if res.config.chart_ratio <= 0.95 {
                draw_text_aligned_opt_width(ui, &res.config.watermark, 0., (-top * 0.98 + (1. - p) * 0.4) / res.config.chart_ratio, (0.5, 1.), 0.25 * scale_ratio / res.config.chart_ratio, semi_white(0.5 * c.a), 2.0 * aspect_ratio);
            }
        };
        let hw = 0.003;
        let height = eps * 1.0;
        let offset = self.chart.offset + self.info_offset + res.config.audio_offset;
        let dest = (res.time - self.exercise_range.start + offset) / (self.exercise_range.end - self.exercise_range.start);
        let dest = (aspect_ratio * 2. * dest as f32).max(0.).min(aspect_ratio * 2.);
        if res.config.render_ui_bar {
            self.chart.with_element(ui, res, UIElement::Bar, Some((-aspect_ratio, top + height / 2.)), Some((-aspect_ratio, top + height / 2.)), |ui, color| {
                //let ct = Vector::new(0., top + height / 2.);
                ui.fill_rect(
                    Rect::new(-aspect_ratio, top, dest, height),
                    Color{ a: color.a * c.a, ..color },
                );
                ui.fill_rect(Rect::new(-aspect_ratio + dest - hw, top, hw * 2., height), Color::new(0.95, 0.95, 0.95, color.a * c.a));
            });
        }
        Ok(())
    }

    fn overlay_ui(&mut self, ui: &mut Ui, tm: &mut TimeManager) -> Result<()> {
        let c = semi_white(self.res.alpha);
        let res = &mut self.res;
        for pos in &self.touch_points {
            ui.fill_circle(pos.0, pos.1, 0.04, Color { a: 0.4, ..BLUE });
        }
        #[cfg(feature = "play")]
        if res.config.shake_play_mode && matches!(self.state, State::Playing) {
            let acc = GYRO.lock().unwrap().get_current_acceleration().abs();
            res.shake_play_mode_deque.push_back((tm.real_time(), acc));
            while res.shake_play_mode_deque.front().is_some_and(|it| tm.real_time() - it.0 > 1.0) {
                res.shake_play_mode_deque.pop_front();
            }
            let none_gt_1 = res.shake_play_mode_deque.iter().all(|(_, a)| *a <= 1.0);
            if none_gt_1 && !is_key_down(KeyCode::Enter) {
                res.shake_play_paused = true;
                if !tm.paused() {
                    tm.pause();
                    self.music.pause()?;
                    debug!("Shake Mode: Paused");
                }
                ui.text(tl!("shake-to-resume"))
                    .pos(0., 0.)
                    .anchor(0.5, 0.5)
                    .size(1.0)
                    .color(semi_white(1.0))
                    .draw();
                return Ok(());
            } else if tm.paused() && res.shake_play_paused {
                res.shake_play_paused = false;
                tm.resume();
                self.music.play()?;
                debug!("Shake Mode: Resumed");
            }
        }
        if tm.paused() {
            let o = if matches!(self.mode, GameMode::Exercise | GameMode::TweakOffset) { -0.3 } else { 0. };
            let s = 0.06;
            let w = 0.05;
            let no_retry = self.mode == GameMode::NoRetry;
            draw_texture_ex(
                &res.icon_back,
                -s * 3. - w,
                -s + o,
                c,
                DrawTextureParams {
                    dest_size: Some(vec2(s * 2., s * 2.)),
                    ..Default::default()
                },
            );
            draw_texture_ex(
                &res.icon_retry,
                -s,
                -s + o,
                if no_retry { semi_white(res.alpha * 0.6) } else { c },
                DrawTextureParams {
                    dest_size: Some(vec2(s * 2., s * 2.)),
                    ..Default::default()
                },
            );
            draw_texture_ex(
                &res.icon_resume,
                s + w,
                -s + o,
                c,
                DrawTextureParams {
                    dest_size: Some(vec2(s * 2., s * 2.)),
                    ..Default::default()
                },
            );
            if res.config.interactive {
                let mut clicked = None;
                for touch in Judge::get_touches(1.0, res.resolution_ratio) {
                    if touch.phase != TouchPhase::Started {
                        continue;
                    }
                    let p = touch.position;
                    let p = Point::new(p.x, p.y);
                    for i in -1..=1 {
                        let ct = Point::new((s * 2. + w) * i as f32, o);
                        let d = p - ct;
                        if d.x.abs() <= s && d.y.abs() <= s {
                            clicked = Some(i);
                            break;
                        }
                    }
                }
                if no_retry && clicked == Some(0) {
                    clicked = None;
                }
                if clicked.is_some_and(|it| it != -1) && (tm.speed - res.config.speed as f64).abs() > 1e-3 {
                    reset_music_speed!(self, res, tm);
                }
                match clicked {
                    Some(-1) => {
                        self.should_exit = true;
                    }
                    Some(0) => {
                        reset!(self, res, tm);
                        self.pause_rewind = PauseRewind {
                            time: Some(tm.now()),
                            duration: Some(0.1),
                            dim: false,
                        };
                        res.disable_hit_fx = true;
                    }
                    Some(1) => {
                        if self.mode == GameMode::Exercise && tm.now() > self.exercise_range.end && self.exercise_range.end - 0.1 < res.track_length {
                            tm.seek_to(self.exercise_range.start);
                            self.music.seek_to(self.exercise_range.start)?;
                        }
                        self.music.fade_in(0.5)?;
                        let now = tm.now();
                        tm.speed = res.config.speed as _;
                        tm.resume();
                        tm.seek_to(now - 1.);
                        self.music.seek_to(now - 1.)?;
                        self.pause_rewind = PauseRewind {
                            time: Some(tm.now()),
                            duration: Some(1.0),
                            dim: true
                        };
                        self.res.disable_hit_fx = true;
                    }
                    _ => {}
                }
            }
            if matches!(self.mode, GameMode::Exercise | GameMode::TweakOffset) {
                let asp = self.touch_scale();
                let track_length = self.res.track_length as f32;
                for touch in ui.ensure_touches() {
                    touch.position *= asp;
                }
                if matches!(self.mode, GameMode::Exercise) {
                    ui.scope(|ui| {
                        ui.dx(0.3);
                        ui.dy(-0.3);
                        ui.slider(tl!("speed"), 0.1..2.0, 0.05, &mut self.res.config.speed, Some(0.5));
                    });
                }
                ui.dy(0.06);
                let hw = 0.7;
                let h = 0.06;
                let eh = 0.12;
                let rad = 0.03;
                let sp = self.offset_chart().min(0.);
                ui.fill_rect(Rect::new(-hw, -h, hw * 2., h * 2.), Color::new(0.4, 0.4, 0.4, 1.));
                let st = -hw + (self.exercise_range.start - sp) as f32 / (self.res.track_length - sp) as f32 * hw * 2.;
                let en = -hw + (self.exercise_range.end - sp) as f32 / (self.res.track_length - sp) as f32 * hw * 2.;
                let t = tm.now();
                let cur = -hw + (t - sp) as f32 / (self.res.track_length - sp) as f32 * hw * 2.;
                ui.fill_rect(Rect::new(st, -h, en - st, h * 2.), Color::new(0.6, 0.6, 0.6, 1.));
                ui.fill_rect(Rect::new(st, -eh, 0., eh + h).feather(0.005), Color::new(0.66, 0.78, 0.98, 1.));
                ui.fill_circle(st, -eh, rad, Color::new(0.66, 0.78, 0.98, 1.));
                if self.exercise_press.is_none() {
                    let r = ui.rect_to_global(Rect::new(st, -eh, 0., 0.).feather(rad));
                    self.exercise_press = Judge::get_touches(1.0, self.res.resolution_ratio)
                        .iter()
                        .find(|it| it.phase == TouchPhase::Started && r.contains(it.position))
                        .map(|it| (-1, it.id));
                }
                ui.fill_rect(Rect::new(en, -h, 0., eh + h).feather(0.005), Color::new(1., 0.34, 0.54, 1.));
                ui.fill_circle(en, eh, rad, Color::new(1., 0.34, 0.54, 1.));
                if self.exercise_press.is_none() {
                    let r = ui.rect_to_global(Rect::new(en, eh, 0., 0.).feather(rad));
                    self.exercise_press = Judge::get_touches(1.0, self.res.resolution_ratio)
                        .iter()
                        .find(|it| it.phase == TouchPhase::Started && r.contains(it.position))
                        .map(|it| (1, it.id));
                }
                ui.fill_rect(Rect::new(cur, -h, 0., h * 2.).feather(0.005), Color::new(0.9, 0.9, 0.9, 1.));
                ui.fill_circle(cur, 0., rad, Color::new(0.95, 0.95, 0.95, 1.));
                if self.exercise_press.is_none() {
                    let r = ui.rect_to_global(Rect::new(cur, 0., 0., 0.).feather(rad));
                    self.exercise_press = Judge::get_touches(1.0, self.res.resolution_ratio)
                        .iter()
                        .find(|it| it.phase == TouchPhase::Started && r.contains(it.position))
                        .map(|it| (0, it.id));
                }
                ui.text(fmt_time(tm.now())).pos(0., -0.23).anchor(0.5, 0.).size(0.8).draw();
                if self.pause_rewind.time.is_some() {
                    self.exercise_press = None;
                }
                if let Some((ctrl, id)) = &self.exercise_press {
                    if let Some(touch) = Judge::get_touches(1.0, self.res.resolution_ratio).iter().rfind(|it| it.id == *id) {
                        let x = touch.position.x;
                        let p = (x + hw) / (hw * 2.) * (self.res.track_length - sp) as f32 + sp as f32;
                        let p = if track_length - sp as f32 <= 3. || *ctrl == 0 {
                            p.clamp(sp as f32, track_length)
                        } else {
                            p.clamp(
                                if *ctrl == -1 { sp as f32 } else { self.exercise_range.start as f32 + 3. },
                                if *ctrl == -1 {
                                    self.exercise_range.end as f32 - 3.
                                } else {
                                    track_length
                                },
                            )
                        };
                        if *ctrl == 0 {
                            tm.seek_to(p as f64);
                            self.music.pause()?;
                            self.music.seek_to(p as f64)?;
                        } else {
                            *(if *ctrl == -1 {
                                &mut self.exercise_range.start
                            } else {
                                &mut self.exercise_range.end
                            }) = p as f64;
                        }
                        if matches!(touch.phase, TouchPhase::Cancelled | TouchPhase::Ended) {
                            self.exercise_press = None;
                        }
                    }
                }
                ui.dy(0.2);
                let r = ui.text(tl!("to")).size(0.8).anchor(0.5, 0.).draw();
                let mut tx = ui
                    .text(fmt_time(self.exercise_range.start))
                    .pos(r.x - 0.02, 0.)
                    .anchor(1., 0.)
                    .size(0.8)
                    .color(BLACK);
                let re = tx.measure();
                self.exercise_btns.0.set(tx.ui, re);
                tx.ui
                    .fill_rect(re.feather(0.01), Color::new(1., 1., 1., if self.exercise_btns.0.touching() { 0.5 } else { 1. }));
                tx.draw();

                let mut tx = ui
                    .text(fmt_time(self.exercise_range.end))
                    .pos(r.right() + 0.02, 0.)
                    .size(0.8)
                    .color(BLACK);
                let re = tx.measure();
                self.exercise_btns.1.set(tx.ui, re);
                tx.ui
                    .fill_rect(re.feather(0.01), Color::new(1., 1., 1., if self.exercise_btns.1.touching() { 0.5 } else { 1. }));
                tx.draw();
                for touch in ui.ensure_touches() {
                    touch.position /= asp;
                }
            }
        }
        if let PauseRewind {
            time: Some(time),
            duration: Some(duration),
            dim
        } = self.pause_rewind {
            let dt = tm.now() - time;
            let t = duration - dt;
            if t <= 0. {
                self.pause_rewind = PauseRewind {
                    time: None,
                    duration: None,
                    dim: false
                };
                self.res.disable_hit_fx = false;
            } else if dim {
                let a = (t / duration).clamp(0.0, 1.0) as f32 * PAUSE_BACKGROUND_ALPHA;
                let h = 1. / self.res.aspect_ratio;
                draw_rectangle(-1., -h, 2., h * 2., Color::new(0., 0., 0., a));
                ui.text((t.ceil() as i32).to_string()).anchor(0.5, 0.5).size(1.).color(c.with_alpha(a)).draw();
            }
        }
        Ok(())
    }

    fn interactive(res: &Resource, state: &State) -> bool {
        res.config.interactive && matches!(state, State::Playing)
    }

    fn offset(&self, speed: f64) -> f64 {
        self.chart.offset +
        self.info_offset +
        (self.res.config.audio_offset +
        if self.res.config.auto_tweak_offset {
            get_audio_latency(&self.res.audio)
        } else {
            0.
        }) * speed
    }

    fn offset_chart(&self) -> f64 {
        self.chart.offset + self.info_offset
    }

    fn tweak_offset(&mut self, ui: &mut Ui, ita: bool, tm: &mut TimeManager) -> Result<()> {
        let width = 0.60;
        let height = 0.3;
        ui.scope(|ui| -> Result<()> {
            ui.dx(1. - width - 0.02);
            ui.dy(ui.top - height - 0.02);
            ui.fill_rect(Rect::new(0., 0., width, height), Color { r: 0.13, g: 0.13, b: 0.13, a: 0.5 });
            ui.dy(0.02);
            ui.text(tl!("adjust-offset")).pos(width / 2., 0.).anchor(0.5, 0.).size(0.7).draw();

            ui.dx(width / 1.22);
            if ui.button("cancel", Rect::new(0.02, 0., 0.06, 0.06), "×") {
                self.next_scene = Some(NextScene::PopWithResult(Box::new(Some(self.info_offset))));
            }
            ui.dx(-width / 1.22);

            ui.dy(0.20);
            let r = ui
                .text(format!("{}ms", (self.info_offset * 1000.).round() as i32))
                .pos(width / 2., 0.)
                .anchor(0.5, 0.)
                .size(0.6)
                .no_baseline()
                .draw();
            let d = 0.18;
            let mut bpm_list = self.chart.bpm_list.borrow_mut();
            let beat = (15. / bpm_list.now_bpm(tm.now())).clamp(0.020, 0.500);
            if ui.button("lg_sub", Rect::new(d, r.center().y, 0., 0.).feather(0.030), "-") && ita {
                self.info_offset -= beat;
                if let Some(sfx_vec) = &mut self.sfx_vec {
                    offset_sfx_list(sfx_vec, &mut self.res, -beat)?;
                }
            }
            if ui.button("lg_add", Rect::new(width - d, r.center().y, 0., 0.).feather(0.030), "+") && ita {
                self.info_offset += beat;
                if let Some(sfx_vec) = &mut self.sfx_vec {
                    offset_sfx_list(sfx_vec, &mut self.res, beat)?;
                }
            }
            let d = 0.11;
            if ui.button("sm_sub", Rect::new(d, r.center().y, 0., 0.).feather(0.026), "-") && ita {
                self.info_offset -= 0.010;
                if let Some(sfx_vec) = &mut self.sfx_vec {
                    offset_sfx_list(sfx_vec, &mut self.res, -0.010)?;
                }
            }
            if ui.button("sm_add", Rect::new(width - d, r.center().y, 0., 0.).feather(0.026), "+") && ita {
                self.info_offset += 0.010;
                if let Some(sfx_vec) = &mut self.sfx_vec {
                    offset_sfx_list(sfx_vec, &mut self.res, 0.010)?;
                }
            }
            let d = 0.047;
            if ui.button("ti_sub", Rect::new(d, r.center().y, 0., 0.).feather(0.023), "-") && ita {
                self.info_offset -= 0.001;
                if let Some(sfx_vec) = &mut self.sfx_vec {
                    offset_sfx_list(sfx_vec, &mut self.res, -0.001)?;
                }
            }
            if ui.button("ti_add", Rect::new(width - d, r.center().y, 0., 0.).feather(0.023), "+") && ita {
                self.info_offset += 0.001;
                if let Some(sfx_vec) = &mut self.sfx_vec {
                    offset_sfx_list(sfx_vec, &mut self.res, 0.001)?;
                }
            }
            /*ui.dy(0.10);
            let pad = 0.02;
            let spacing = 0.01;
            let mut r = Rect::new(pad, 0., (width - pad * 2. - spacing * 2.) / 3., 0.06);
            if ui.button("cancel", r, tl!("offset-cancel")) {
                self.next_scene = Some(NextScene::PopWithResult(Box::new(None::<f32>)));
            }
            r.x += r.w + spacing;
            if ui.button("reset", r, tl!("offset-reset")) {
                self.info_offset = 0.;
            }
            r.x += r.w + spacing;
            if ui.button("save", r, tl!("offset-save")) {
                //self.res.info.offset = self.info_offset;
                self.next_scene = Some(NextScene::PopWithResult(Box::new(Some(self.info_offset))));
            }*/
            Ok(())
        })?;
        ui.scope(|ui| {
            ui.dx(1. - width * 0.97);
            ui.dy(ui.top - height * 0.75);
            ui.slider(tl!("speed"), 0.1..2.0, 0.05, &mut self.res.config.speed, Some(0.40));
            if (tm.speed - self.res.config.speed as f64).abs() > 1e-3 {
                reset_music_speed!(self, &mut self.res, tm);
                tm.resume();
                self.music.play().ok();
            }
        });
        Ok(())
    }
}

impl Scene for GameScene {
    fn enter(&mut self, tm: &mut TimeManager, target: Option<RenderTarget>) -> Result<()> {
        #[cfg(target_arch = "wasm32")]
        on_game_start();
        self.music = Self::new_music(&mut self.res)?;
        self.res.camera.render_target = target;
        tm.speed = self.res.config.speed as _;
        tm.adjust_time = self.res.config.adjust_time;
        reset!(self, self.res, tm);
        set_camera(&self.res.camera);
        self.first_in = true;
        self.first_update_time = tm.real_time();
        if let Some(ref upload_fn) = self.upload_fn {
            self.refresh_task = Some((upload_fn.refresh)());
        }
        self.pause_rewind = PauseRewind {
            time: Some(tm.now()),
            duration: Some(0.1),
            dim: false,
        };
        self.res.disable_hit_fx = true;
        Ok(())
    }

    fn pause(&mut self, tm: &mut TimeManager) -> Result<()> {
        self.res.audio.close()?;
        if !tm.paused() {
            self.pause_rewind = PauseRewind {
                time: None,
                duration: None,
                dim: false
            };
            self.music.pause()?;
            tm.pause();
        }
        Ok(())
    }

    fn resume(&mut self, tm: &mut TimeManager) -> Result<()> {
        self.res.audio.start()?;
        if tm.paused() && !matches!(self.state, State::Playing) {
            tm.resume();
        }
        Ok(())
    }

    fn focus_pause(&mut self, tm: &mut TimeManager) -> Result<()> {
        if !self.res.config.autoplay() && !tm.paused() {
            self.pause_rewind = PauseRewind {
                time: None,
                duration: None,
                dim: false
            };
            self.music.fade_out(0.3)?;
            tm.pause();
        }
        Ok(())
    }

    fn focus_resume(&mut self, tm: &mut TimeManager) -> Result<()> {
        if tm.paused() && !matches!(self.state, State::Playing) {
            tm.resume();
        }
        Ok(())
    }

    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        let time = tm.now();
        self.res.audio.recover_if_needed()?;
        if matches!(self.state, State::Playing) && time < self.res.track_length {
            tm.update(self.music.position());
        }
        if self.mode == GameMode::Exercise && tm.now() > self.exercise_range.end && self.exercise_range.end < self.res.track_length - 0.1 && !tm.paused() {
            let state = self.state.clone();
            reset!(self, self.res, tm);
            self.state = state;
            tm.seek_to(self.exercise_range.start);
            tm.pause();
            self.music.fade_out(0.3)?;
        }
        if tm.paused() && self.res.config.rotation_mode {
            GYRO.lock().unwrap().reset_gyroscope();
        }
        let time = match self.state {
            State::Starting => {
                let refresh_done = self.refresh_task.as_ref().map_or(true, |t| t.ok());
                if (time >= Self::BEFORE_DURATION || !self.res.config.enter_animation) && refresh_done {
                    self.res.alpha = 1.;
                    self.state = State::BeforeMusic;
                    tm.reset();
                    tm.seek_to(self.exercise_range.start);
                    self.last_update_time = tm.real_time();
                    if self.first_in && self.mode == GameMode::Exercise {
                        //tm.pause();
                        //self.music.pause()?;
                        self.first_in = false;
                    }
                    tm.now()
                } else {
                    if self.res.config.rotation_mode {
                        GYRO.lock().unwrap().reset_gyroscope();
                    }
                    if self.res.config.enter_animation {
                        self.res.alpha = 1. - (1. - time / Self::BEFORE_TIME).clamp(0., 1.).powi(3) as f32;
                    } else {
                        self.res.alpha = 1.;
                    };
                    self.exercise_range.start
                }
            }
            State::BeforeMusic => {
                if time >= 0.0 {
                    self.music.seek_to(time)?;
                    self.music.play()?;
                    self.state = State::Playing;
                }
                time
            }
            State::Playing => {
                let is_ending = time >= self.res.track_length + WAIT_TIME;
                #[cfg(feature = "play")]
                let is_ending = is_ending || self.res.health.state.track_failed;
                if is_ending {
                    self.music.pause()?;
                    self.state = State::Ending;
                }
                time
            }
            State::Ending => {
                let t = time - self.res.track_length - WAIT_TIME;
                let is_ending = t >= Self::WAIT_AFTER_TIME;
                #[cfg(feature = "play")]
                let is_ending = is_ending || self.res.health.state.track_failed;
                if is_ending {
                    #[cfg(feature = "play")]
                    let track_complete = self.res.health.state.now_health >= self.res.health.config.complete_health && !self.res.health.state.track_failed;
                    #[cfg(not(feature = "play"))]
                    let track_complete = true;
                    #[cfg(feature = "play")]
                    let track_failed = self.res.health.state.track_failed;
                    #[cfg(not(feature = "play"))]
                    let track_failed = false;
                    if self.res.config.autoplay() && !track_failed {
                        self.judge.commit_all(&mut self.chart);
                    }
                    let result = self.judge.result(track_complete);
                    let mut record_data = None;
                    // TODO strengthen the protection
                    #[cfg(feature = "closed")]
                    if let Some(upload_fn) = &self.upload_fn {
                        if !self.res.config.offline_mode
                            && !self.res.config.autoplay()
                            && (self.res.config.speed - 1.0).abs() < 1e-3
                            && track_complete
                            && self.judge.is_vaild()
                            && (result.std - crate::judge::LIMIT_BAD as f32).abs() > 1e-3
                        {
                            if let Some(player) = &self.player {
                                if let Some(chart) = &self.res.info.guid {
                                    record_data = Some(encode_record(self));
                                }
                            }
                        }
                    }
                    let record = if self.res.config.autoplay()
                        || (self.res.config.speed - 1.0).abs() > 1e-3
                        || self.mode == GameMode::Replay
                    {
                        None
                    } else {
                        Some(SimpleRecord {
                            score: result.score as _,
                            accuracy: result.accuracy as _,
                            full_combo: result.max_combo == result.num_of_notes,
                            track_complete,
                        })
                    };
                    if self.judge.replay_recorder.is_some() {
                        let data = self.judge.stop_recording(&self.chart, self.res.config.speed);
                        *LAST_REPLAY.lock().unwrap() = Some((self.res.info.name.clone(), data));
                    }
                    self.next_scene = match self.mode {
                        GameMode::Normal | GameMode::Exercise | GameMode::NoRetry | GameMode::View | GameMode::Replay => Some(NextScene::Overlay(Box::new(EndingScene::new(
                            self.res.background.clone(),
                            self.res.illustration.clone(),
                            self.res.player.clone(),
                            self.res.icons.clone(),
                            self.res.icon_retry.clone(),
                            self.res.icon_proceed.clone(),
                            self.res.info.clone(),
                            self.judge.result(track_complete),
                            self.res.challenge_icons[self.res.config.challenge_color.clone() as usize].clone(),
                            &self.res.config,
                            self.res.res_pack.endings.clone(),
                            self.upload_fn.clone(),
                            self.player.as_ref().map(|it| it.rks),
                            record_data,
                            record,
                        )?))),
                        GameMode::TweakOffset => Some(NextScene::PopWithResult(Box::new(Some(self.info_offset)))),
                    };
                }
                self.res.alpha = 1. - (t / AFTER_TIME).clamp(0., 1.).powi(2) as f32;
                self.res.track_length + WAIT_TIME
            }
        };

        let time = if self.mode == GameMode::TweakOffset {
            (time - self.offset_chart()).max(0.)
        } else {
            (time - self.offset(self.res.config.speed as f64)).max(0.)
        };
        self.res.time = time;
        if !tm.paused() && (self.res.config.autoplay() || self.pause_rewind.time.is_none()) && self.mode != GameMode::View {
            self.gl.quad_gl.viewport(self.res.camera.viewport);

            let angle = if self.res.config.rotation_mode {
                GYRO.lock().unwrap().get_angle()
            } else {
                0.
            };

            if self.mode == GameMode::Replay {
                self.judge.update_replay(&mut self.res, &mut self.chart, &mut self.bad_notes);
            } else {
                self.judge.update(&mut self.res, &mut self.chart, &mut self.bad_notes, -angle);
            }
            if self.mode == GameMode::Replay {
                let now = tm.now();
                for it in self.judge.replay_touches() {
                    self.replay_trails.push((now, it.position));
                }
                self.replay_trails.retain(|(t, _)| now - *t <= TRAIL_DURATION);
            }
            #[cfg(feature = "play")]
            if self.res.config.health_mode.is_some() && matches!(self.state, State::Playing) && matches!(self.mode, GameMode::Normal | GameMode::NoRetry | GameMode::View | GameMode::Replay) {
                self.res.health.update(time as f32);
            }
            self.gl.quad_gl.viewport(None);
        }
        if let Some(update) = &mut self.update_fn {
            update(self.res.time, &mut self.res, &mut self.judge);
        }
        let counts = self.judge.counts();
        self.res.judge_line_color = if counts[2] + counts[3] == 0 {
            if counts[1] == 0 {
                self.res.res_pack.info.line_perfect()
            } else {
                self.res.res_pack.info.line_good()
            }
        } else {
            WHITE
        };
        self.chart.update(&mut self.res);
        let res = &mut self.res;
        if res.config.interactive && is_key_pressed(KeyCode::Space) {
            if tm.paused() {
                if matches!(self.state, State::Playing) {
                    let now = tm.now();
                    if (tm.speed - res.config.speed as f64).abs() > 1e-3 {
                        reset_music_speed!(self, res, tm);
                    }
                    self.music.seek_to(now)?;
                    self.music.fade_in(0.5)?;
                    tm.seek_to(now);
                    tm.resume();
                    self.pause_rewind = PauseRewind {
                        time: Some(now),
                        duration: Some(0.1),
                        dim: false
                    };
                    res.disable_hit_fx = true;
                }
            } else if matches!(self.state, State::Playing) && !self.pause_rewind.dim { // State::BeforeMusic
                if !self.music.paused() {
                    self.music.fade_out(0.3)?;
                }
                self.pause_rewind = PauseRewind {
                    time: None,
                    duration: None,
                    dim: false
                };
                tm.pause();
            }
        }
        if Self::interactive(res, &self.state) {
            if is_key_pressed(KeyCode::Left) {
                res.time -= 2.;
                let dst = (self.music.position() - 2.).max(0.);
                self.music.seek_to(dst)?;
                tm.seek_to(dst);
            }
            if is_key_pressed(KeyCode::Right) {
                res.time += 5.;
                let dst = (self.music.position() + 5.).min(res.track_length);
                self.music.seek_to(dst)?;
                tm.seek_to(dst);

                self.pause_rewind = PauseRewind {
                    time: Some(tm.now()),
                    duration: Some(0.1),
                    dim: false
                };
                res.disable_hit_fx = true;
            }
            if is_key_pressed(KeyCode::Q) {
                self.should_exit = true;
            }
        }
        for effect in &mut self.effects {
            effect.update(&self.res);
        }
        if let Some((id, text)) = take_input() {
            let offset = self.offset_chart().min(0.);
            match id.as_str() {
                "exercise_start" => {
                    if let Some(t) = parse_time(&text) {
                        if !(offset..self.res.track_length.min(self.exercise_range.end - 3.).max(offset)).contains(&t) {
                            show_message(tl!("ex-time-out-of-range")).error();
                        } else {
                            self.exercise_range.start = t;
                            show_message(tl!("ex-time-set")).ok();
                        }
                    } else {
                        show_message(tl!("ex-invalid-format")).error();
                    }
                }
                "exercise_end" => {
                    if let Some(t) = parse_time(&text) {
                        if !((self.exercise_range.start + 3.).max(offset).min(self.res.track_length)..self.res.track_length).contains(&t) {
                            show_message(tl!("ex-time-out-of-range")).error();
                        } else {
                            self.exercise_range.end = t;
                            show_message(tl!("ex-time-set")).ok();
                        }
                    } else {
                        show_message(tl!("ex-invalid-format")).error();
                    }
                }
                _ => return_input(id, text),
            }
        }
        Ok(())
    }

    fn touch(&mut self, tm: &mut TimeManager, touch: &Touch) -> Result<bool> {
        if self.mode == GameMode::Exercise && tm.paused() {
            let touch = Touch {
                position: touch.position * self.touch_scale(),
                ..touch.clone()
            };
            if self.exercise_btns.0.touch(&touch) {
                request_input("exercise_start", &fmt_time(self.exercise_range.start), tl!("ex-time-start"));
                return Ok(true);
            }
            if self.exercise_btns.1.touch(&touch) {
                request_input("exercise_end", &fmt_time(self.exercise_range.end), tl!("ex-time-end"));
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        let res = &mut self.res;

        let time = tm.now();
        let p = match self.state {
            State::Starting => {
                if time < Self::BEFORE_DURATION {
                    1. - (1. - time / Self::BEFORE_DURATION)
                } else {
                    1.
                }
            }
            State::BeforeMusic => 1.,
            State::Ending | State::Playing => {
                let t = time - res.track_length;
                1. - (t / Self::BEFORE_DURATION).clamp(0., 1.)
            }
        };
        let ratio = if res.config.chart_ratio != 1. && res.config.enter_animation {
            1. + (res.config.chart_ratio - 1.) * ease_in_out_quartic(p as f32)
        } else {
            res.config.chart_ratio
        };

        if res.config.dynamic_resolution_mode && !matches!(self.state, State::Starting) && res.frame_times.len() > 5 {
            let min = crate::ext::round_to_step((ui.viewport.3 as f32 / 2.).min(540.) / ui.viewport.3 as f32, 0.05);
            let now = tm.real_time();
            let fps = res.frame_times.len();
            let mut frame_times = res.frame_times.iter().rev();
            let now_fps_5 = 1. / frame_times
                .next()
                .zip(frame_times.nth(4))
                .map_or(0., |(latest, oldest)| (latest - oldest) / 5.);
            let mut frame_times = res.frame_times.iter().rev();
            let now_fps_2 = 1. / frame_times
                .next()
                .zip(frame_times.nth(1))
                .map_or(0., |(latest, oldest)| (latest - oldest) / 2.);
            {
                if fps > 0 {
                    res.best_fps = res.best_fps.max(fps);
                    if now_fps_5 as f64 / res.best_fps as f64 <= 0.3 || now_fps_2 as f64 / res.best_fps as f64 <= 0.2 {
                        res.dynamic_resolution_ratio = min;
                        res.last_adjustment = now;
                    } else if now_fps_5 as f64 / res.best_fps as f64 <= 0.7 && now - res.last_adjustment > 0.05 {
                        res.dynamic_resolution_ratio = (res.dynamic_resolution_ratio - 0.1).max(min);
                        res.last_adjustment = now;
                    } else if fps as f64 / res.best_fps as f64 >= 0.9 && now - res.last_adjustment > 0.4 && res.dynamic_resolution_ratio < 1.0 {
                        res.dynamic_resolution_ratio = (res.dynamic_resolution_ratio + 0.1).min(1.0);
                        res.last_adjustment = now;
                    }
                }
            }
        } else {
            res.dynamic_resolution_ratio = 1.0;
        }
        if res.config.low_resolution_mode {
            res.resolution_ratio = res.dynamic_resolution_ratio * 0.5;
        } else {
            res.resolution_ratio = res.dynamic_resolution_ratio;
        }

        if res.update_size(ui.viewport) || self.mode == GameMode::View {
            set_camera(&res.camera);
        }

        let msaa = res.config.sample_count > 1;

        // camera setup
        let ui_viewport = res.parse_resolution_ratio(ui.viewport);
        let vp = res.camera.viewport.unwrap_or(ui_viewport);
        let viewport_window = Some(ui_viewport);
        let viewport_chart = if res.chart_target.is_some() {
            Some((vp.0 - ui_viewport.0, vp.1 - ui_viewport.1, vp.2, vp.3))
        } else {
            res.camera.viewport
        };

        let asp2_window = ui_viewport.2 as f32 / ui_viewport.3 as f32;
        let asp2_chart = vp.2 as f32 / vp.3 as f32;
        let asp2_ui = vp.3 as f32 / vp.2 as f32;
        let asp2_ui_window = ui_viewport.3 as f32 / ui_viewport.2 as f32;

        let chart_onto = res
            .chart_target
            .as_ref()
            .map(|it| if msaa { it.input() } else { it.output() })
            .or(res.camera.render_target.clone());

        let h = 1. / res.aspect_ratio;
        set_camera(&Camera2D {
            zoom: vec2(1., asp2_window),
            viewport: if res.chart_target.is_some() { None } else { viewport_window },
            render_target: chart_onto.clone(),
            ..Default::default()
        });
        if !res.config.preserve_framebuffer {
            clear_background(BLACK);
        }
        if res.config.render_bg {
            draw_background(&res.background, res.config.render_bg_dim);
        }

        if res.config.render_bg_dim && res.config.chart_ratio >= 1. {
            let dim_alpha = 0.7;
            //let alpha = res.alpha * (1. - dim_alpha) + dim_alpha;    
            let dim = Color::new(0.1, 0.1, 0.1, dim_alpha * res.alpha);
            let x_range = vp.0 as f32 / ui_viewport.2 as f32;
            let y_range = vp.1 as f32 / vp.3 as f32;
            draw_rectangle(-1., -h,x_range * 2., h * 2., dim); // Left
            draw_rectangle(1., -h,-x_range * 2., h * 2., dim); // Right
            draw_rectangle(-1., -h,2., -y_range * 2., dim); // Top
            draw_rectangle(-1., h,2., y_range * 2., dim); // Bottom
            draw_rectangle(x_range * 2. - 1., -h, (1. - x_range * 2.) * 2., h * 2., Color::new(0., 0., 0., res.alpha * res.info.background_dim));
        }

        let chart_zoom = if res.config.chart_ratio < 1. { vec2(asp2_chart / asp2_window * ratio, asp2_chart * ratio) } else { vec2(1. * ratio, asp2_chart * ratio) };
        let chart_viewport = if res.config.chart_ratio < 1. { viewport_window } else { viewport_chart };

        if res.config.render_bg_dim && res.config.chart_ratio < 1. {
            set_camera(&Camera2D {
                zoom: chart_zoom,
                viewport: chart_viewport,
                render_target: chart_onto.clone(),
                ..Default::default()
            });
            self.gl.quad_gl.render_pass(chart_onto.as_ref().map(|it| it.render_pass.raw_miniquad_id()));
            draw_rectangle(-1., -h, 2., h * 2., Color::new(0., 0., 0., res.alpha * res.info.background_dim));
        }

        let angle = if res.config.rotation_mode {
            GYRO.lock().unwrap().get_angle()
        } else {
            0.
        };
        set_camera(&Camera2D {
            zoom: chart_zoom,
            viewport: chart_viewport.map(|(x, y, w, h)| {
                if res.info.fold_animation && matches!(self.state, State::Starting) {
                    let scale_x = (1. - (1. - time / Self::BEFORE_TIME).clamp(0., 1.).powi(3)).powf(2.0) as f32;
                    let new_w = (w as f32 * scale_x).round() as i32;
                    let dx = (w - new_w) / 2;
                    (x + dx, y, new_w, h)
                } else if res.info.fold_animation && matches!(self.state, State::Ending) {
                    let t = time - res.track_length - WAIT_TIME;
                    let scale_x = (1. - (t / (Self::WAIT_AFTER_TIME)).clamp(0., 1.).powi(2)).powf(2.0) as f32;
                    let new_w = (w as f32 * scale_x).round() as i32;
                    let dx = (w - new_w) / 2;
                    (x + dx, y, new_w, h)
                } else {
                    (x, y, w, h)
                }
            }),
            rotation: angle.to_degrees(),
            render_target: chart_onto.clone(),
            ..Default::default()
        });
        self.gl.quad_gl.render_pass(chart_onto.as_ref().map(|it| it.render_pass.raw_miniquad_id()));
        self.chart.render(ui, res);

        if self.mode == GameMode::Replay {
            let now = tm.now();
            for (t, pos) in &self.replay_trails {
                let age = ((now - t) / TRAIL_DURATION).clamp(0., 1.) as f32;
                let alpha = (1. - age) * 0.35;
                let radius = TRAIL_RADIUS * (1. - age * 0.5);
                ui.fill_circle(pos.x, pos.y, radius, Color::new(1., 0.45, 0.5, alpha));
            }
            let touches = self.judge.replay_touches();
            for it in &touches {
                ui.fill_circle(it.position.x, it.position.y, TRAIL_RADIUS * 1.6, Color::new(1., 0.45, 0.5, 0.3));
                ui.fill_circle(it.position.x, it.position.y, TRAIL_RADIUS, Color::new(1., 1., 1., 0.55));
            }
        }

        self.gl.quad_gl.render_pass(
            res.chart_target
                .as_ref()
                .map(|it| it.output().render_pass.raw_miniquad_id())
                .or_else(|| Some(res.camera.render_pass()?.raw_miniquad_id())),
        );

        self.bad_notes.retain(|dummy| dummy.render(res));
        let t = tm.real_time();
        let dt = (t - std::mem::replace(&mut self.last_update_time, t)) as f32;
        if res.config.particle {
            res.emitter.draw(dt);
        }

        if !res.no_effect {
            set_camera(&Camera2D {
                zoom: vec2(1., asp2_chart),
                render_target: chart_onto.clone(),
                viewport: Some(ui_viewport),
                ..Default::default()
            });
            for effect in &self.chart.extra.effects {
                effect.render(res);
            }
        }
        
        {
            set_camera(&Camera2D {
                zoom: if res.config.chart_ratio < 1. { vec2(asp2_ui_window * ratio, 1. * ratio) } else { vec2(asp2_ui * ratio, 1. * ratio) },
                viewport: chart_viewport,
                render_target: self.res.chart_target.as_ref().map(|it| it.output()).or(self.res.camera.render_target.clone()),
                ..Default::default()
            });
            self.ui(ui, tm)?;
        }

        if !self.res.no_effect && !self.effects.is_empty() {
            set_camera(&Camera2D {
                zoom: vec2(1., asp2_window),
                render_target: chart_onto.clone(),
                viewport: Some(ui_viewport),
                ..Default::default()
            });
            for effect in &self.effects {
                effect.render(&mut self.res);
            }
        }

        {
            set_camera(&Camera2D {
                zoom: vec2(1., 1.),
                viewport: viewport_window,
                render_target: self.res.chart_target.as_ref().map(|it| it.output()).or(self.res.camera.render_target.clone()),
                ..Default::default()
            });
            if tm.paused() {
                draw_rectangle(-1., -1., 2., 2., Color::new(0., 0., 0., PAUSE_BACKGROUND_ALPHA));
            }
        }

        {
            set_camera(&Camera2D {
                zoom: vec2(1., asp2_window),
                viewport: viewport_window,
                render_target: self.res.chart_target.as_ref().map(|it| it.output()).or(self.res.camera.render_target.clone()),
                ..Default::default()
            });
            if self.mode == GameMode::TweakOffset {
                self.tweak_offset(ui, Self::interactive(&self.res, &self.state), tm)?;
            }
            if self.res.config.touch_debug {
                for touch in Judge::get_touches(1.0, self.res.resolution_ratio) {
                    ui.fill_circle(touch.position.x, touch.position.y, 0.04, Color { a: 0.4, ..RED });
                }
            }
        }
        
        {
            set_camera(&Camera2D {
                zoom: vec2(1., asp2_chart),
                viewport: viewport_chart,
                render_target: self.res.chart_target.as_ref().map(|it| it.output()).or(self.res.camera.render_target.clone()),
                ..Default::default()
            });
            self.overlay_ui(ui, tm)?;
        }

        if !self.res.no_effect || msaa || self.res.config.low_resolution_mode || self.res.config.dynamic_resolution_mode {
            // render the texture onto screen
            if let Some(target) = &self.res.chart_target {
                self.gl.flush();
                self.gl.quad_gl.viewport(None);
                set_camera(&Camera2D {
                    zoom: vec2(1., asp2_window),
                    render_target: self.res.camera.render_target.clone(),
                    viewport: Some(ui.viewport),
                    ..Default::default()
                });
                draw_texture_ex(
                    &target.output().texture,
                    -1.,
                    -ui.top,
                    WHITE,
                    DrawTextureParams {
                        dest_size: Some(vec2(2., ui.top * 2.)),
                        ..Default::default()
                    },
                );
            }
        } else {
            self.gl.flush();
        }

        if self.res.config.auto_tweak_offset || self.res.config.dynamic_resolution_mode {
            push_frame_time(&mut self.res.frame_times, tm.real_time());
        }
        
        Ok(())
    }

    fn next_scene(&mut self, tm: &mut TimeManager) -> NextScene {
        if self.should_exit {
            let _ = self.music.pause();
            if tm.paused() {
                tm.resume();
            }
            tm.speed = 1.0;
            tm.adjust_time = false;
            match self.mode {
                GameMode::Normal | GameMode::Exercise | GameMode::NoRetry | GameMode::View | GameMode::Replay => NextScene::Pop,
                GameMode::TweakOffset => NextScene::PopWithResult(Box::new(None::<f32>)),
            }
        } else if let Some(next_scene) = self.next_scene.take() {
            if tm.paused() {
                tm.resume();
            }
            tm.speed = 1.0;
            tm.adjust_time = false;
            next_scene
        } else {
            NextScene::None
        }
    }
}
