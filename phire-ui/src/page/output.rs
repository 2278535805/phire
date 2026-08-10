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
    ui::{DRectButton, Ui},
};
use sasa::{AudioManager, Renderer};
#[cfg(target_os = "windows")]
use sasa::BackendStreamInfo::Wasapi;
#[cfg(not(any(target_os = "android", target_os = "windows")))]
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

const FREQ_VALUES: &[f32] = &[80.0, 200.0, 440.0, 1000.0];
const AMP_VALUES: &[f32] = &[0.0, 0.25, 0.5, 0.75, 1.0];

const FREQ_DEFAULT_IDX: usize = 2; // 440 Hz
const AMP_DEFAULT_IDX: usize = 3; // 0.5

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

    freq_btn: DRectButton,
    amp_btn: DRectButton,
    freq_idx: usize,
    amp_idx: usize,
    waveform_idx: usize,
    wf_btns: Vec<DRectButton>,
    active: bool,
    play_btn: DRectButton,

    #[cfg(any(target_os = "android", target_os = "windows"))]
    compat_btn: DRectButton,
    #[cfg(any(target_os = "android", target_os = "windows"))]
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
            frequency: Mutex::new(FREQ_VALUES[FREQ_DEFAULT_IDX] as f64),
            amplitude: Mutex::new(AMP_VALUES[AMP_DEFAULT_IDX]),
            waveform: Mutex::new(Waveform::Sine),
            active: Mutex::new(false),
        });

        let renderer = ToneRenderer::new(params.clone());
        audio.add_renderer(renderer)?;

        Ok(Self {
            audio: Some(audio),
            params,

            tm: TimeManager::new(1., false),

            freq_btn: DRectButton::new(),
            amp_btn: DRectButton::new(),
            freq_idx: FREQ_DEFAULT_IDX,
            amp_idx: AMP_DEFAULT_IDX,
            waveform_idx: 0,
            wf_btns: (0..6).map(|_| DRectButton::new()).collect(),
            active: false,
            play_btn: DRectButton::new(),

            #[cfg(any(target_os = "android", target_os = "windows"))]
            compat_btn: DRectButton::new(),
            #[cfg(any(target_os = "android", target_os = "windows"))]
            compat: config.audio_compatibility,
            audio_buffer_size_btn: DRectButton::new(),
            #[cfg(not(any(target_os = "android", target_os = "windows")))]
            base_buffer_size: Some(64),
            #[cfg(any(target_os = "android", target_os = "windows"))]
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

        if self.freq_btn.touch(touch, t) {
            self.freq_idx = (self.freq_idx + 1) % FREQ_VALUES.len();
            *self.params.frequency.lock().unwrap() = FREQ_VALUES[self.freq_idx] as f64;
            return Ok(true);
        }
        if self.amp_btn.touch(touch, t) {
            self.amp_idx = (self.amp_idx + 1) % AMP_VALUES.len();
            *self.params.amplitude.lock().unwrap() = AMP_VALUES[self.amp_idx];
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
        #[cfg(any(target_os = "android", target_os = "windows"))]
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
                    #[cfg(not(any(target_os = "android", target_os = "windows")))]
                    Some(n) if n == base_buffer * 4 => Some(base_buffer * 8),
                    #[cfg(not(any(target_os = "android", target_os = "windows")))]
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
                    #[cfg(target_os = "windows")]
                    Wasapi(info) => {
                        if let Some(sample_rate) = info.sample_rate {
                            if let Some(min_period_hns) = info.min_period_hns {
                                let min_buffer = min_period_hns * sample_rate / 10000000;
                                self.base_buffer_size = Some(min_buffer);
                            }
                        }
                    }
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
            #[cfg(not(any(target_os = "android", target_os = "windows")))]
            let mut y = r.center().y - 0.30;
            #[cfg(any(target_os = "android", target_os = "windows"))]
            let mut y = r.center().y - 0.35; // compat_btn

            let active = *self.params.active.lock().unwrap();
            let start_label = if active { tl!("stop") } else { tl!("start") };
            let btn_rect = Rect::new(right_center - 0.22, y, 0.44, 0.08);
            self.play_btn.render_text(ui, btn_rect, t, c.a, start_label, 0.45, active);
            ui.text(tl!("title"))
                .pos(right_center + 0.25, y + 0.04)
                .anchor(0., 0.5)
                .size(0.32)
                .color(Color::new(1., 1., 1., 0.7 * c.a))
                .draw();

            #[cfg(any(target_os = "android", target_os = "windows"))]
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
            y += 0.10;

            let freq = FREQ_VALUES[self.freq_idx];
            *self.params.frequency.lock().unwrap() = freq as f64;
            let freq_text = format!("{:.0} Hz", freq);
            let freq_rect = Rect::new(right_center - 0.22, y, 0.44, 0.08);
            self.freq_btn.render_text(ui, freq_rect, t, c.a, freq_text, 0.45, false);
            ui.text(tl!("freq"))
                .pos(right_center + 0.25, y + 0.04)
                .anchor(0., 0.5)
                .size(0.32)
                .color(Color::new(1., 1., 1., 0.7 * c.a))
                .draw();
            y += 0.10;

            *self.params.amplitude.lock().unwrap() = AMP_VALUES[self.amp_idx];
            let amp_text = format!("{:.0}%", AMP_VALUES[self.amp_idx] * 100.0);
            let amp_rect = Rect::new(right_center - 0.22, y, 0.44, 0.08);
            self.amp_btn.render_text(ui, amp_rect, t, c.a, amp_text, 0.45, false);
            ui.text(tl!("volume"))
                .pos(right_center + 0.25, y + 0.04)
                .anchor(0., 0.5)
                .size(0.32)
                .color(Color::new(1., 1., 1., 0.7 * c.a))
                .draw();
            y += 0.12;

            ui.text(tl!("waveform"))
                .pos(right_center, y)
                .anchor(0.5, 0.)
                .size(0.5)
                .color(Color::new(1., 1., 1., 0.6 * c.a))
                .draw();
            y += 0.06;

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
            y += 0.08;

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

                let mut warn_str = Vec::new();
                match audio.stream_info() {
                    #[cfg(target_os = "android")]
                    Oboe(info) => {
                        if let Some(latency_millis) = info.latency_millis {
                            if latency_millis < 0.0 {
                                warn_str.push(tl!("failed-audio-write"));
                            }
                        }
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
                            if xrun_count > 8 {
                                warn_str.push(tl!("found-xrun"));
                            }
                        }
                    },
                    #[cfg(not(any(target_os = "android", target_os = "windows")))]
                    Cpal(info) => {
                        if let Some(settings_buffer_size) = info.settings.buffer_size {
                            if let Some(actual_frames_per_callback) = info.actual_frames_per_callback {
                                if settings_buffer_size != actual_frames_per_callback {
                                    warn_str.push(tl!("failed-buffer-size"));
                                }
                            }
                        }
                    }
                    #[cfg(target_os = "windows")]
                    Wasapi(info) => {
                        if let Some(settings_buffer_size) = info.settings.buffer_size {
                            if let Some(actual_frames_per_callback) = info.actual_frames_per_callback {
                                if settings_buffer_size != actual_frames_per_callback {
                                    warn_str.push(tl!("failed-buffer-size"));
                                }
                            }
                        }
                        if let Some(settings_sample_rate) = info.settings.sample_rate {
                            if let Some(actual_sample_rate) = info.sample_rate {
                                if settings_sample_rate != actual_sample_rate {
                                    warn_str.push(tl!("failed-sample-rate"));
                                }
                            }
                        }
                    }
                    #[allow(unreachable_patterns)] _ => {}, // TODO: OHOS
                }
                if !warn_str.is_empty() {
                    let mut warn_str_merge = String::new();
                    for (i, s) in warn_str.iter().enumerate() {
                        warn_str_merge.push_str(s);
                        if i > 0 && (i + 1) % 2 == 0 {
                            warn_str_merge.push_str("\n");
                        } else {
                            warn_str_merge.push_str("  ");
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
            push_frame_time(&mut self.frame_times, self.tm.real_time());
        });
        Ok(())
    }
}
