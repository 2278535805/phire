phire::tl_file!("output");

use super::{Page, SharedState};
use crate::get_data;
use crate::{get_data_mut, save_data};
use anyhow::Result;
use macroquad::prelude::*;
use phire::{
    ext::{create_audio_manger, get_audio_latency, get_frame_latency, push_frame_time, screen_aspect, semi_black},
    scene::show_message,
    time::TimeManager,
    ui::{DRectButton, Slider, Ui},
};
use sasa::{AudioManager, Renderer};
#[cfg(not(target_os = "android"))]
use sasa::BackendStreamInfo::Cpal;
#[cfg(target_os = "android")]
use sasa::{BackendStreamInfo::Oboe, backend::oboe::{PerformanceMode, SharingMode}};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

#[derive(Clone, Copy, PartialEq)]
enum Waveform {
    Sine,
    Square,
    Sawtooth,
    Triangle,
    WhiteNoise,
    Sweep,
}

const WAVEFORMS: &[(&str, Waveform)] = &[
    ("Sine", Waveform::Sine),
    ("Square", Waveform::Square),
    ("Saw", Waveform::Sawtooth),
    ("Triangle", Waveform::Triangle),
    ("Noise", Waveform::WhiteNoise),
    ("Sweep", Waveform::Sweep),
];

const FREQ_MIN: f32 = 20.0;
const FREQ_MAX: f32 = 8000.0;
const FREQ_DEFAULT: f32 = 440.0;

struct ToneParams {
    frequency: Mutex<f64>,
    amplitude: Mutex<f32>,
    waveform: Mutex<Waveform>,
    active: Mutex<bool>,
}

struct ToneRenderer {
    phase: f64,
    sweep_phase: f64,
    sweep_dir: f64,
    sample_rate: u32,
    params: Arc<ToneParams>,
}

impl ToneRenderer {
    fn new(params: Arc<ToneParams>) -> Self {
        Self {
            phase: 0.0,
            sweep_phase: 100.0,
            sweep_dir: 1.0,
            sample_rate: 48000,
            params,
        }
    }

    fn generate(&mut self, data: &mut [f32], is_stereo: bool) {
        if !*self.params.active.lock().unwrap() {
            data.fill(0.0);
            return;
        }
        let freq = *self.params.frequency.lock().unwrap();
        let amp = *self.params.amplitude.lock().unwrap();
        let wf = *self.params.waveform.lock().unwrap();
        let sr = self.sample_rate as f64;
        let step = if wf == Waveform::Sweep {
            self.sweep_phase += self.sweep_dir * 50.0 / sr;
            if self.sweep_phase >= 8000.0 {
                self.sweep_dir = -1.0;
            }
            if self.sweep_phase <= 50.0 {
                self.sweep_dir = 1.0;
            }
            self.sweep_phase.clamp(50.0, 8000.0) / sr
        } else {
            freq / sr
        };

        let chunk_size = if is_stereo { 2 } else { 1 };
        for chunk in data.chunks_exact_mut(chunk_size) {
            let val = match wf {
                Waveform::Sine => (self.phase * 2.0 * std::f64::consts::PI).sin() as f32,
                Waveform::Square => {
                    if (self.phase * 2.0 * std::f64::consts::PI).sin() >= 0.0 {
                        1.0
                    } else {
                        -1.0
                    }
                }
                Waveform::Sawtooth => ((self.phase % 1.0) * 2.0 - 1.0) as f32,
                Waveform::Triangle => ((self.phase % 1.0) * 4.0 - 1.0).abs().mul_add(2.0, -1.0) as f32,
                Waveform::WhiteNoise => macroquad::rand::rand() as f32 / u32::MAX as f32 * 2.0 - 1.0,
                Waveform::Sweep => (self.phase * 2.0 * std::f64::consts::PI).sin() as f32,
            };
            let sample = (val * amp).clamp(-1.0, 1.0);
            chunk[0] = sample;
            if is_stereo {
                chunk[1] = sample;
            }
            self.phase = (self.phase + step) % 1.0;
        }
    }
}

impl Renderer for ToneRenderer {
    fn alive(&self) -> bool {
        true
    }
    fn render_mono(&mut self, sample_rate: u32, data: &mut [f32]) {
        self.sample_rate = sample_rate;
        self.generate(data, false);
    }
    fn render_stereo(&mut self, sample_rate: u32, data: &mut [f32]) {
        self.sample_rate = sample_rate;
        self.generate(data, true);
    }
}

pub struct OutputPage {
    audio: Option<AudioManager>,
    params: Arc<ToneParams>,

    tm: TimeManager,

    freq_slider: Slider,
    amp_slider: Slider,
    freq_norm: f32,
    amp_norm: f32,
    waveform_idx: usize,
    wf_btns: Vec<DRectButton>,
    active: bool,
    play_btn: DRectButton,

    #[cfg(target_os = "android")]
    compat_btn: DRectButton,
    #[cfg(target_os = "android")]
    compat: bool,
    audio_buffer_size_btn: DRectButton,
    base_buffer_size: Option<u32>,
    rebuild_needed: bool,

    frame_times: VecDeque<f64>,
}

impl OutputPage {
    pub async fn new() -> Result<Self> {
        let config = &get_data().config;
        let mut audio = create_audio_manger(config)?;

        let params = Arc::new(ToneParams {
            frequency: Mutex::new(FREQ_DEFAULT as f64),
            amplitude: Mutex::new(0.5),
            waveform: Mutex::new(Waveform::Sine),
            active: Mutex::new(false),
        });

        let renderer = ToneRenderer::new(params.clone());
        audio.add_renderer(renderer)?;

        let freq_norm = freq_to_norm(FREQ_DEFAULT);
        let amp_norm = 0.5;

        Ok(Self {
            audio: Some(audio),
            params,

            tm: TimeManager::new(1., false),

            freq_slider: Slider::new(0.0..1.0, 0.001),
            amp_slider: Slider::new(0.0..1.0, 0.01),
            freq_norm,
            amp_norm,
            waveform_idx: 0,
            wf_btns: (0..6).map(|_| DRectButton::new()).collect(),
            active: false,
            play_btn: DRectButton::new(),

            #[cfg(target_os = "android")]
            compat_btn: DRectButton::new(),
            #[cfg(target_os = "android")]
            compat: config.audio_compatibility,
            audio_buffer_size_btn: DRectButton::new(),
            #[cfg(not(target_os = "android"))]
            base_buffer_size: Some(64),
            #[cfg(target_os = "android")]
            base_buffer_size: None,
            rebuild_needed: false,

            frame_times: VecDeque::new(),
        })
    }

    fn rebuild_audio(&mut self) -> Result<()> {
        drop(self.audio.take());
        let config = &get_data().config;
        let mut audio = create_audio_manger(config)?;
        let renderer = ToneRenderer::new(self.params.clone());
        audio.add_renderer(renderer)?;
        self.audio = Some(audio);
        Ok(())
    }
}

impl Page for OutputPage {
    fn can_play_bgm(&self) -> bool {
        false
    }

    fn label(&self) -> std::borrow::Cow<'static, str> {
        "OUTPUT".into()
    }

    fn exit(&mut self) -> Result<()> {
        Ok(())
    }

    fn enter(&mut self, _s: &mut SharedState) -> Result<()> {
        self.tm.reset();
        Ok(())
    }

    fn pause(&mut self) -> Result<()> {
        self.tm.pause();
        Ok(())
    }

    fn resume(&mut self) -> Result<()> {
        self.tm.resume();
        Ok(())
    }

    fn touch(&mut self, touch: &Touch, s: &mut SharedState) -> Result<bool> {
        let t = s.t;

        if self.freq_slider.touch(touch, t, &mut self.freq_norm).is_some() {
            let freq = norm_to_freq(self.freq_norm) as f64;
            *self.params.frequency.lock().unwrap() = freq;
            return Ok(true);
        }
        if self.amp_slider.touch(touch, t, &mut self.amp_norm).is_some() {
            *self.params.amplitude.lock().unwrap() = self.amp_norm;
            return Ok(true);
        }
        if self.play_btn.touch(touch, t) {
            self.active ^= true;
            *self.params.active.lock().unwrap() = self.active;
            return Ok(true);
        }
        for (i, btn) in self.wf_btns.iter_mut().enumerate() {
            if btn.touch(touch, t) {
                self.waveform_idx = i;
                *self.params.waveform.lock().unwrap() = WAVEFORMS[i].1;
                return Ok(true);
            }
        }
        #[cfg(target_os = "android")]
        if self.compat_btn.touch(touch, t) {
            let config = &mut get_data_mut().config;
            config.audio_compatibility ^= true;
            self.compat = config.audio_compatibility;
            self.rebuild_needed = true;
            save_data()?;
            return Ok(true);
        }
        if let Some(base_buffer) = self.base_buffer_size {
            if self.audio_buffer_size_btn.touch(touch, t) {
                let config = &mut get_data_mut().config;
                config.audio_buffer_size = match config.audio_buffer_size {
                    None => Some(base_buffer),
                    Some(n) if n == base_buffer => Some(base_buffer * 2),
                    Some(n) if n == base_buffer * 2 => Some(base_buffer * 3),
                    Some(n) if n == base_buffer * 3 => Some(base_buffer * 4),
                    #[cfg(not(target_os = "android"))]
                    Some(n) if n == base_buffer * 4 => Some(base_buffer * 8),
                    #[cfg(not(target_os = "android"))]
                    Some(n) if n == base_buffer * 8 => Some(base_buffer * 16),
                    _ => None,
                };
                self.rebuild_needed = true;
                save_data()?;
                return Ok(true);
            }
        }

        Ok(false)
    }

    fn update(&mut self, _s: &mut SharedState) -> Result<()> {
        if let Some(audio) = &mut self.audio {
            audio.recover_if_needed()?;
        }

        if self.rebuild_needed {
            self.rebuild_needed = false;
            if let Err(e) = self.rebuild_audio() {
                show_message(format!("{}", e)).error();
            }
        }

        Ok(())
    }

    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let t = s.t;
        let aspect = 1. / screen_aspect();
        s.render_fader(ui, |ui, c| {
            let lf = -0.97;
            let mut r = ui.content_rect();
            r.w += r.x - lf;
            r.x = lf;
            ui.fill_rect(r, semi_black(c.a * 0.4));

            if let Some(audio) = self.audio.as_mut() {
                match audio.stream_info() { // TODO: clone()
                    #[cfg(target_os = "android")]
                    Oboe(info) => {
                        if let Some(frames_per_burst) = info.frames_per_burst {
                            self.base_buffer_size = Some(frames_per_burst as u32);
                        }
                    },
                    #[allow(unreachable_patterns)] _ => {}, // TODO: OHOS
                }
                let y = r.y + aspect * 0.05;
                #[cfg(target_os = "android")]
                let left_x = r.x + 0.06 * aspect;
                #[cfg(target_os = "android")]
                let size = 0.42 * aspect;
                #[cfg(not(target_os = "android"))]
                let left_x = r.x + 0.06 * aspect;
                #[cfg(not(target_os = "android"))]
                let size = 0.50 * aspect;
                ui.text(format!("{}", audio.stream_info()))
                    .pos(left_x, y)
                    .anchor(0., 0.)
                    .size(size)
                    .color(Color::new(1., 1., 1., 0.55 * c.a))
                    .multiline()
                    .draw();
            } else {
                ui.text(tl!("audio-error"))
                    .pos(r.left() * 0.5 - 0.05, r.center().y)
                    .anchor(0.5, 0.5)
                    .size(0.5)
                    .color(Color::new(0.9, 0.3, 0.3, c.a))
                    .draw();
            }

            let right_center = r.right() * 0.5  - 0.1;
            #[cfg(not(target_os = "android"))]
            let mut y = r.center().y - aspect * 0.70;
            #[cfg(target_os = "android")]
            let mut y = r.center().y - aspect * 0.75; // compat_btn

            ui.text(tl!("title"))
                .pos(right_center, y)
                .anchor(0.5, 0.)
                .size(0.65)
                .color(Color::new(1., 1., 1., c.a))
                .draw();
            y += 0.10;

            let active = *self.params.active.lock().unwrap();
            let start_label = if active { tl!("stop") } else { tl!("start") };
            let btn_rect = Rect::new(right_center - 0.22, y, 0.44, 0.08);
            self.play_btn.render_text(ui, btn_rect, t, c.a, start_label, 0.45, active);

            #[cfg(target_os = "android")]
            {
                y += 0.10;
                let compat_label = if self.compat {
                    ttl!("switch-on")
                } else {
                    ttl!("switch-off")
                };
                let compat_rect = Rect::new(right_center - 0.22, y, 0.44, 0.08);
                self.compat_btn
                    .render_text(ui, compat_rect, t, c.a, compat_label, 0.45, self.compat);
                ui.text(tl!("compatibility"))
                    .pos(right_center + 0.25, y + 0.04)
                    .anchor(0., 0.5)
                    .size(0.32)
                    .color(Color::new(1., 1., 1., 0.7 * c.a))
                    .draw();
            }
            if self.base_buffer_size.is_some() {
                y += 0.10;
                let config = &get_data().config;
                let text = match config.audio_buffer_size {
                    None => tl!("auto").to_string(),
                    Some(n) => format!("{}", n),
                };
                let buf_rect = Rect::new(right_center - 0.22, y, 0.44, 0.08);
                self.audio_buffer_size_btn
                    .render_text(ui, buf_rect, t, c.a, &text, 0.45, config.audio_buffer_size.is_some());
                ui.text(tl!("buffer-size"))
                    .pos(right_center + 0.25, y + 0.04)
                    .anchor(0., 0.5)
                    .size(0.32)
                    .color(Color::new(1., 1., 1., 0.7 * c.a))
                    .draw();
            }
            y += aspect * 0.20;

            let slider_left = right_center - 0.08 + 0.03;

            let freq = norm_to_freq(self.freq_norm);
            *self.params.frequency.lock().unwrap() = freq as f64;
            self.freq_slider.render(
                ui,
                Rect::new(slider_left, y + aspect * 0.01, 0.55, aspect * 0.035),
                t,
                c,
                self.freq_norm,
                format!("{:.0} Hz", freq),
            );
            y += 0.10;

            *self.params.amplitude.lock().unwrap() = self.amp_norm;
            self.amp_slider.render(
                ui,
                Rect::new(slider_left, y + aspect * 0.01, 0.55, aspect * 0.035),
                t,
                c,
                self.amp_norm,
                format!("{:.0}%", self.amp_norm * 100.0),
            );
            y += aspect * 0.18;

            ui.text(tl!("waveform"))
                .pos(right_center, y)
                .anchor(0.5, 0.)
                .size(0.5)
                .color(Color::new(1., 1., 1., 0.6 * c.a))
                .draw();
            y += 0.08;

            let btn_w = 0.16;
            let btn_h = 0.06;
            let total_w = WAVEFORMS.len() as f32 * btn_w + (WAVEFORMS.len() - 1) as f32 * 0.015;
            let start_x = right_center - total_w / 2.0;
            for (i, (name, _)) in WAVEFORMS.iter().enumerate() {
                let bx = start_x + i as f32 * (btn_w + 0.015);
                let sel = i == self.waveform_idx;
                self.wf_btns[i].render_text(
                    ui,
                    Rect::new(bx, y, btn_w, btn_h),
                    t,
                    c.a,
                    *name,
                    0.30,
                    sel,
                );
            }
            y += 0.10;

            if let Some(audio) = self.audio.as_mut() {
                let audio_latency = get_audio_latency(audio);
                let frame_latency = get_frame_latency(&self.frame_times);
                let latency_text = if audio_latency > 0.0 && frame_latency > 0.0 && frame_latency < 0.25 {
                    format!("{} {:.1} ms + {:.1} ms", tl!("est-latency"), audio_latency * 1000.0, frame_latency * 1000.0)
                } else {
                    format!("{} N/A", tl!("est-latency"))
                };
                ui.text(latency_text)
                    .pos(right_center, y)
                    .anchor(0.5, 0.)
                    .size(0.4)
                    .color(Color::new(1., 1., 1., 0.7 * c.a))
                    .draw();
                match audio.stream_info() {
                    #[cfg(target_os = "android")]
                    Oboe(info) => {
                        let mut warn_str = Vec::new();
                        if matches!(info.settings.sharing_mode, SharingMode::Exclusive) {
                            if let Some(actual_sharing_mode) = info.actual_sharing_mode {
                                if info.settings.sharing_mode != actual_sharing_mode {
                                    warn_str.push(tl!("failed-exclusive"));
                                }
                            }
                        }
                        if matches!(info.settings.performance_mode, PerformanceMode::LowLatency) {
                            if let Some(actual_performance_mode) = info.actual_performance_mode {
                                if info.settings.performance_mode != actual_performance_mode {
                                    warn_str.push(tl!("failed-low-latency"));
                                }
                            }
                        }
                        if matches!(info.settings.performance_mode, PerformanceMode::LowLatency) && info.settings.buffer_size.is_none() {
                            if let Some(actual_buffer_size) = info.actual_buffer_size {
                                if let Some(frames_per_burst) = info.frames_per_burst {
                                    if actual_buffer_size > frames_per_burst * 2 {
                                        warn_str.push(tl!("unexpected-buffer-size"));
                                    }
                                }
                            }
                        }
                        if matches!(info.settings.performance_mode, PerformanceMode::LowLatency) {
                            if let Some(settings_buffer_size) = info.settings.buffer_size {
                                if let Some(actual_buffer_size) = info.actual_buffer_size {
                                    if settings_buffer_size as i32 != actual_buffer_size {
                                        warn_str.push(tl!("failed-buffer-size"));
                                    }
                                }
                            }
                        }
                        if let Some(xrun_count) = info.xrun_count {
                            if xrun_count > 0 {
                                warn_str.push(tl!("found-xrun"));
                            }
                        }
                        if !warn_str.is_empty() {
                            let mut warn_str_merge = String::new();
                            for (i, s) in warn_str.iter().enumerate() {
                                warn_str_merge.push_str(s);
                                if i > 0 && (i + 1) % 3 == 0 {
                                    warn_str_merge.push_str("\n");
                                } else {
                                    warn_str_merge.push_str(" ");
                                }
                            }
                            y += 0.06;
                            ui.text(warn_str_merge)
                                .pos(right_center, y)
                                .anchor(0.0, 0.0)
                                .size(0.4)
                                .color(Color::new(1., 1., 1., 0.7 * c.a))
                                .centered_multiline()
                                .draw();
                        }
                    },
                    #[cfg(not(target_os = "android"))]
                    Cpal(info) => {
                        let mut warn_str = Vec::new();
                        if let Some(settings_buffer_size) = info.settings.buffer_size {
                            if let Some(actual_frames_per_callback) = info.actual_frames_per_callback {
                                if settings_buffer_size != actual_frames_per_callback {
                                    warn_str.push(tl!("failed-buffer-size"));
                                }
                            }
                        }
                        if !warn_str.is_empty() {
                            let mut warn_str_merge = String::new();
                            for (i, s) in warn_str.iter().enumerate() {
                                warn_str_merge.push_str(s);
                                if i > 0 && (i + 1) % 3 == 0 {
                                    warn_str_merge.push_str("\n");
                                } else {
                                    warn_str_merge.push_str(" ");
                                }
                            }
                            y += 0.06;
                            ui.text(warn_str_merge)
                                .pos(right_center, y)
                                .anchor(0.0, 0.0)
                                .size(0.4)
                                .color(Color::new(1., 1., 1., 0.7 * c.a))
                                .centered_multiline()
                                .draw();
                        }
                    }
                    #[allow(unreachable_patterns)] _ => {}, // TODO: OHOS
                }
            }
            if get_data().config.auto_tweak_offset {
                push_frame_time(&mut self.frame_times, self.tm.real_time());
            }
        });
        Ok(())
    }
}

fn freq_to_norm(freq: f32) -> f32 {
    let log_min = FREQ_MIN.ln();
    let log_max = FREQ_MAX.ln();
    ((freq.ln() - log_min) / (log_max - log_min)).clamp(0.0, 1.0)
}

fn norm_to_freq(norm: f32) -> f32 {
    let log_min = FREQ_MIN.ln();
    let log_max = FREQ_MAX.ln();
    let log_val = log_min + (log_max - log_min) * norm;
    log_val.exp().clamp(FREQ_MIN, FREQ_MAX)
}
