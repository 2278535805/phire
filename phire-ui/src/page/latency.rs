phire::tl_file!("latency");

use super::{Page, SharedState};
use crate::get_data;
use anyhow::Result;
use macroquad::prelude::*;
use phire::{
    ext::{create_audio_manger, push_frame_time, screen_aspect, semi_black},
    time::TimeManager,
    ui::Ui,
};
use sasa::{AudioManager, AudioRecorder, Recorder, Renderer};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering}},
};

struct RingBuffer {
    data: Vec<f32>,
    start_pos: u64,
    capacity: usize,
}

impl RingBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            data: Vec::with_capacity(capacity),
            start_pos: 0,
            capacity,
        }
    }

    fn push_samples(&mut self, samples: &[f32]) {
        self.data.extend_from_slice(samples);
        if self.data.len() > self.capacity {
            let excess = self.data.len() - self.capacity;
            self.data.drain(..excess);
            self.start_pos += excess as u64;
        }
    }

    fn read_range(&self, from_pos: u64, len: usize) -> Vec<f32> {
        let end = self.start_pos + self.data.len() as u64;
        if from_pos >= end {
            return vec![];
        }
        let offset = if from_pos < self.start_pos {
            0
        } else {
            (from_pos - self.start_pos) as usize
        };
        let available = self.data.len() - offset;
        let count = len.min(available);
        self.data[offset..offset + count].to_vec()
    }
}

struct BeepRenderer {
    trigger: Arc<AtomicBool>,
    sample_count: Arc<AtomicU64>,
    frequency: f32,
    phase: f64,
    remaining: u32,
}

impl BeepRenderer {
    fn new(trigger: Arc<AtomicBool>, sample_count: Arc<AtomicU64>, frequency: f32) -> Self {
        Self {
            trigger,
            sample_count,
            frequency,
            phase: 0.0,
            remaining: 0,
        }
    }
}

impl Renderer for BeepRenderer {
    fn alive(&self) -> bool {
        true
    }
    fn render_mono(&mut self, sample_rate: u32, data: &mut [f32]) {
        if self.trigger.swap(false, Ordering::Relaxed) {
            self.remaining = (sample_rate as f32 * 0.08) as u32;
            self.sample_count.store(0, Ordering::Relaxed);
        }
        let sr = sample_rate as f64;
        for sample in data.iter_mut() {
            if self.remaining > 0 {
                let val = ((self.phase * 2.0 * std::f64::consts::PI).sin() as f32 * 0.7).clamp(-1.0, 1.0);
                *sample = val;
                self.phase = (self.phase + self.frequency as f64 / sr) % 1.0;
                self.remaining -= 1;
            } else {
                *sample = 0.0;
            }
        }
        self.sample_count.fetch_add(data.len() as u64, Ordering::Relaxed);
    }
    fn render_stereo(&mut self, sample_rate: u32, data: &mut [f32]) {
        if self.trigger.swap(false, Ordering::Relaxed) {
            self.remaining = (sample_rate as f32 * 0.08) as u32;
            self.sample_count.store(0, Ordering::Relaxed);
        }
        let sr = sample_rate as f64;
        for chunk in data.chunks_exact_mut(2) {
            if self.remaining > 0 {
                let val = ((self.phase * 2.0 * std::f64::consts::PI).sin() as f32 * 0.7).clamp(-1.0, 1.0);
                chunk[0] = val;
                chunk[1] = val;
                self.phase = (self.phase + self.frequency as f64 / sr) % 1.0;
                self.remaining -= 1;
            } else {
                chunk[0] = 0.0;
                chunk[1] = 0.0;
            }
        }
        self.sample_count.fetch_add(data.len() as u64 / 2, Ordering::Relaxed);
    }
}

struct TapRecorder {
    buffer: Arc<Mutex<RingBuffer>>,
    position: Arc<AtomicU64>,
    sample_rate: Arc<AtomicU32>,
}

impl TapRecorder {
    fn new(buffer: Arc<Mutex<RingBuffer>>, position: Arc<AtomicU64>, sample_rate: Arc<AtomicU32>) -> Self {
        Self {
            buffer,
            position,
            sample_rate,
        }
    }
}

impl Recorder for TapRecorder {
    fn alive(&self) -> bool {
        true
    }
    fn record_mono(&mut self, sr: u32, data: &[f32]) {
        self.sample_rate.store(sr, Ordering::Relaxed);
        self.buffer.lock().unwrap().push_samples(data);
        self.position.fetch_add(data.len() as u64, Ordering::Relaxed);
    }
    fn record_stereo(&mut self, sample_rate: u32, data: &[f32]) {
        let mono: Vec<f32> = data.chunks_exact(2).map(|c| (c[0] + c[1]) * 0.5).collect();
        self.record_mono(sample_rate, &mono);
    }
}

#[derive(PartialEq)]
enum AnalysisPhase {
    Idle,
    WaitingForBeep,
    Scheduled,
    Done,
}

pub struct LatencyPage {
    audio: AudioManager,

    recorder: Option<AudioRecorder>,
    trigger: Arc<AtomicBool>,
    sample_count: Arc<AtomicU64>,
    position: Arc<AtomicU64>,
    sample_rate: Arc<AtomicU32>,
    buffer: Arc<Mutex<RingBuffer>>,

    tm: TimeManager,

    result_ms: Option<f64>,
    measuring: bool,
    trigger_pos: Option<u64>,
    trigger_time: Option<std::time::Instant>,
    phase: AnalysisPhase,

    viz_samples: Vec<f32>,
    viz_slow: Vec<f32>,
    viz_fast: Vec<f32>,
    viz_threshold: Vec<f32>,
    viz_edges: Vec<usize>,
    viz_sr: u32,

    measurement_count: usize,
    latency_sum: f64,
    latency_min: f64,
    latency_max: f64,
    latency_values: Vec<f64>,
    unreliable: bool,

    frame_times: VecDeque<f64>,
}

impl LatencyPage {
    pub async fn new() -> Result<Self> {
        let config = &get_data().config;
        let mut audio = create_audio_manger(config)?;

        let trigger = Arc::new(AtomicBool::new(false));
        let sample_count = Arc::new(AtomicU64::new(0));
        let position = Arc::new(AtomicU64::new(0));
        let sample_rate = Arc::new(AtomicU32::new(48000));

        let beep = BeepRenderer::new(trigger.clone(), sample_count.clone(), 1000.0);
        audio.add_renderer(beep)?;

        let buffer = Arc::new(Mutex::new(RingBuffer::new(48000 * 5)));

        let recorder = create_recorder(buffer.clone(), position.clone(), sample_rate.clone()).ok();

        let tm = TimeManager::new(1., false);

        Ok(Self {
            audio,
            recorder,
            trigger,
            sample_count,
            position,
            sample_rate,
            buffer,
            tm,
            result_ms: None,
            measuring: false,
            trigger_pos: None,
            trigger_time: None,
            phase: AnalysisPhase::Idle,
            viz_samples: Vec::new(),
            viz_slow: Vec::new(),
            viz_fast: Vec::new(),
            viz_threshold: Vec::new(),
            viz_edges: Vec::new(),
            viz_sr: 48000,
            measurement_count: 0,
            latency_sum: 0.0,
            latency_min: f64::MAX,
            latency_max: 0.0,
            latency_values: Vec::new(),
            unreliable: false,
            frame_times: VecDeque::new(),
        })
    }

    fn trigger_measurement(&mut self) {
        if self.phase != AnalysisPhase::Idle && self.phase != AnalysisPhase::Done {
            return;
        }
        self.result_ms = None;
        self.measuring = true;
        self.phase = AnalysisPhase::WaitingForBeep;
        self.viz_samples.clear();
        self.viz_slow.clear();
        self.viz_fast.clear();
        self.viz_threshold.clear();
        self.viz_edges.clear();
        self.trigger_pos = Some(self.position.load(Ordering::Relaxed));
        self.trigger_time = Some(std::time::Instant::now());
        self.sample_count.store(0, Ordering::Relaxed);
        self.trigger.store(true, Ordering::Relaxed);
    }

    fn analyze(&mut self) {
        let sr = self.sample_rate.load(Ordering::Relaxed);
        let sr_f = sr as f64;
        if sr == 0 {
            self.phase = AnalysisPhase::Done;
            self.measuring = false;
            return;
        }

        let buf = self.buffer.lock().unwrap();
        let trigger_pos = self.trigger_pos.unwrap_or(0);
        let pre_samples = (sr_f * 0.2) as usize;
        let post_samples = (sr_f * 1.2) as usize;
        let from = trigger_pos.saturating_sub(pre_samples as u64);
        let total = pre_samples + post_samples;

        let samples = buf.read_range(from, total);
        drop(buf);

        self.viz_sr = sr;
        self.viz_edges.clear();

        if samples.len() < 1024 {
            self.viz_samples = samples;
            self.result_ms = Some(-3.0);
            self.phase = AnalysisPhase::Done;
            self.measuring = false;
            return;
        }

        let hp = high_pass(&samples, sr_f, 0.95);

        let events_from_hp =
            apply_envelope_and_scan(&hp, sr_f, &mut self.viz_fast, &mut self.viz_slow, &mut self.viz_threshold);

        if events_from_hp.len() == 2 {
            self.viz_samples = hp;
            self.viz_edges = events_from_hp;
        } else {
            let avg = average_filter(&hp);
            let events_from_avg =
                apply_envelope_and_scan(&avg, sr_f, &mut self.viz_fast, &mut self.viz_slow, &mut self.viz_threshold);
            if events_from_avg.len() == 2 {
                self.viz_samples = avg;
                self.viz_edges = events_from_avg;
            } else {
                let gentle_hp = high_pass(&samples, sr_f, 0.80);
                let events_from_gentle =
                    apply_envelope_and_scan(&gentle_hp, sr_f, &mut self.viz_fast, &mut self.viz_slow, &mut self.viz_threshold);
                self.viz_samples = gentle_hp;
                self.viz_edges = events_from_gentle;
            }
        }

        if self.viz_edges.len() >= 2 {
            let latency_samples = self.viz_edges[1] - self.viz_edges[0];
            let latency_ms = latency_samples as f64 / sr_f * 1000.0;
            self.result_ms = Some(latency_ms);

            let diff = self.latency_max - self.latency_min;
            let avg = self.latency_sum / self.measurement_count as f64;
            if self.measurement_count > 3 && (latency_ms - avg).abs() > diff * 2.0 {
                self.unreliable = true;
            } else {
                self.unreliable = false;
                self.measurement_count += 1;
                self.latency_sum += latency_ms;
                self.latency_values.push(latency_ms);
                if latency_ms < self.latency_min {
                    self.latency_min = latency_ms;
                }
                if latency_ms > self.latency_max {
                    self.latency_max = latency_ms;
                }
            }
        } else if self.viz_edges.len() == 1 {
            self.result_ms = Some(-1.0);
        } else {
            self.result_ms = Some(-2.0);
        }

        self.phase = AnalysisPhase::Done;
        self.measuring = false;
    }
}

impl Page for LatencyPage {
    fn can_play_bgm(&self) -> bool {
        false
    }

    fn label(&self) -> std::borrow::Cow<'static, str> {
        "LATENCY".into()
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

    fn touch(&mut self, touch: &Touch, _s: &mut SharedState) -> Result<bool> {
        let x = touch.position.x;
        let y = touch.position.y * screen_aspect();
        if touch.phase == TouchPhase::Started && (-0.95..0.95).contains(&x) && (-0.50..0.95).contains(&y)
        {
            self.trigger_measurement();
        }
        Ok(false)
    }

    fn update(&mut self, _s: &mut SharedState) -> Result<()> {
        self.audio.recover_if_needed()?;
        if let Some(ref mut rec) = self.recorder {
            rec.recover_if_needed()?;
        }

        if self.phase == AnalysisPhase::WaitingForBeep {
            if let Some(t) = self.trigger_time {
                if t.elapsed().as_secs_f64() > 1.2 {
                    self.phase = AnalysisPhase::Scheduled;
                    self.analyze();
                }
            }
        }

        if is_key_pressed(KeyCode::Space) {
            self.trigger_measurement();
        }

        push_frame_time(&mut self.frame_times, self.tm.real_time());
        Ok(())
    }

    fn render(&mut self, ui: &mut Ui, s: &mut SharedState) -> Result<()> {
        let aspect = 1. / screen_aspect();
        s.render_fader(ui, |ui, c| {
            let lf = -0.97;
            let mut r = ui.content_rect();
            r.w += r.x - lf;
            r.x = lf;
            ui.fill_rect(r, semi_black(c.a * 0.4));
            let ct = r.center();

            let mut y = ct.y - aspect * 0.6;
            ui.text(tl!("title"))
                .pos(ct.x, y)
                .anchor(0.5, 0.)
                .size(0.7)
                .color(Color::new(1., 1., 1., c.a))
                .draw();
            y += aspect * 0.2;

            if self.recorder.is_none() {
                ui.text(tl!("no-mic"))
                    .pos(ct.x, y)
                    .anchor(0.5, 0.5)
                    .size(0.5)
                    .color(Color::new(1., 0.5, 0.3, c.a))
                    .draw();
            } else {
                ui.text(tl!("tap-hint"))
                    .pos(ct.x, y)
                    .anchor(0.5, 0.)
                    .size(0.4)
                    .color(Color::new(1., 1., 1., 0.6 * c.a))
                    .draw();
                y += aspect * 0.12;

                // let tap_size_h = 0.15;
                // let tap_area = Rect::new(ct.x - 0.3, y, 0.6, tap_size_h);
                // let tap_color = if self.measuring {
                //     Color::new(0.85, 0.55, 0.10, c.a)
                // } else {
                //     Color::new(0.15, 0.50, 0.20, 0.7 * c.a)
                // };
                // ui.fill_rect(tap_area, tap_color);
                // let label = if self.measuring {
                //     tl!("measuring")
                // } else {
                //     tl!("tap-here")
                // };
                // ui.text(label)
                //     .pos(ct.x, tap_area.center().y)
                //     .anchor(0.5, 0.5)
                //     .size(0.5)
                //     .color(Color::new(1., 1., 1., c.a))
                //     .draw();
                // y += tap_size_h + aspect * 0.1;
                y += aspect * 0.24;

                if let Some(ms) = self.result_ms {
                    if ms == -3.0 {
                        ui.text(tl!("not-enough-audio"))
                            .pos(ct.x, y)
                            .anchor(0.5, 0.)
                            .size(0.4)
                            .color(Color::new(0.9, 0.5, 0.3, c.a))
                            .draw();
                    } else if ms == -2.0 {
                        ui.text(tl!("no-edges"))
                            .pos(ct.x, y)
                            .anchor(0.5, 0.)
                            .size(0.4)
                            .color(Color::new(0.9, 0.5, 0.3, c.a))
                            .draw();
                    } else if ms < 0.0 {
                        ui.text(tl!("one-edge"))
                            .pos(ct.x, y)
                            .anchor(0.5, 0.)
                            .size(0.4)
                            .color(Color::new(0.9, 0.5, 0.3, c.a))
                            .draw();
                    } else {
                        ui.text(format!("{} {:.2} ms", tl!("latency"), ms))
                            .pos(ct.x, y)
                            .anchor(0.5, 0.)
                            .size(0.6)
                            .color(Color::new(0.3, 1., 0.4, c.a))
                            .draw();
                    }
                } else if self.measuring {
                    ui.text(tl!("measuring"))
                        .pos(ct.x, y)
                        .anchor(0.5, 0.)
                        .size(0.6)
                        .color(Color::new(0.85, 0.55, 0.10, c.a))
                        .draw();
                } else {
                    ui.text(tl!("wait-click"))
                        .pos(ct.x, y)
                        .anchor(0.5, 0.)
                        .size(0.6)
                        .color(Color::new(0.3, 1., 0.4, c.a))
                        .draw();
                }

                y += aspect * 0.15;

                if self.measurement_count >= 2 {
                    let avg = self.latency_sum / self.measurement_count as f64;
                        let variance: f64 = self.latency_values.iter().map(|&v| {
                            let d = v - avg;
                            d * d
                        }).sum::<f64>() / self.measurement_count as f64;
                        let std_dev = variance.sqrt();
                    let h = ui.text(format!(
                        "{}: {:.2}  {}: {:.2}  {}: {:.2} {}: {:.2}  (n={})",
                        tl!("min"), self.latency_min,
                        tl!("avg"), avg,
                        tl!("max"), self.latency_max,
                        tl!("std-dev"), std_dev,
                        self.measurement_count
                    ))
                    .pos(ct.x, y)
                    .anchor(0.5, 0.)
                    .size(0.35)
                    .color(Color::new(1., 1., 1., 0.7 * c.a))
                    .draw().h;

                    y += h + aspect * 0.02;

                    if self.measurement_count > 3 && !self.unreliable {
                        let diff = self.latency_max - self.latency_min;
                        if avg < 40. && diff < 10. {
                            ui.text(tl!("level-1"))
                                .pos(ct.x, y)
                                .anchor(0.5, 0.)
                                .size(0.35)
                                .color(Color::new(0.3, 1., 0.4, c.a))
                                .draw();
                        } else if avg < 80. && diff < 20. {
                            ui.text(tl!("level-2"))
                                .pos(ct.x, y)
                                .anchor(0.5, 0.)
                                .size(0.35)
                                .color(Color::new(0.5, 0.9, 0.3, c.a))
                                .draw();
                        } else if avg < 150. && diff < 30. {
                            ui.text(tl!("level-3"))
                                .pos(ct.x, y)
                                .anchor(0.5, 0.)
                                .size(0.35)
                                .color(Color::new(0.9, 0.5, 0.3, c.a))
                                .draw();
                        } else if diff < 40. {
                            ui.text(tl!("level-4"))
                                .pos(ct.x, y)
                                .anchor(0.5, 0.)
                                .size(0.35)
                                .color(Color::new(0.7, 0.7, 0.1, c.a))
                                .draw();
                        } else {
                            ui.text(tl!("level-5"))
                                .pos(ct.x, y)
                                .anchor(0.5, 0.)
                                .size(0.35)
                                .color(Color::new(0.7, 0.1, 0.1, c.a))
                                .draw();
                        }
                    } else if self.unreliable {
                        ui.text(tl!("unreliable"))
                            .pos(ct.x, y)
                            .anchor(0.5, 0.)
                            .size(0.35)
                            .color(Color::new(0.7, 0.1, 0.1, c.a))
                            .draw();
                    } else {
                        ui.text(tl!("need-test"))
                            .pos(ct.x, y)
                            .anchor(0.5, 0.)
                            .size(0.35)
                            .color(Color::new(1., 1., 1., 0.7 * c.a))
                            .draw();
                    }
                } else {
                    ui.text(tl!("need-test"))
                        .pos(ct.x, y)
                        .anchor(0.5, 0.)
                        .size(0.35)
                        .color(Color::new(1., 1., 1., 0.7 * c.a))
                        .draw();
                }
            }

            self.draw_waveform(ui, c, aspect);
        });
        Ok(())
    }
}

impl LatencyPage {
    fn draw_waveform(&self, ui: &mut Ui, c: Color, aspect: f32) {
        let ct = ui.content_rect().center();
        let wf_w = 1.6;
        let wf_h = 0.14;
        let wf_x = -wf_w * 0.5;
        let wf_y = ct.y + aspect * 0.4;
        let mid_y = wf_y + wf_h / 2.0;

        ui.fill_rect(Rect::new(wf_x, wf_y, wf_w, wf_h), Color::new(0.10, 0.50, 0.10, c.a));

        if self.viz_samples.is_empty() {
            if self.measuring {
                ui.text(tl!("measuring"))
                    .pos(0., mid_y)
                    .anchor(0.5, 0.5)
                    .size(0.5)
                    .color(Color::new(0., 0.2, 0., c.a))
                    .draw();
            } else {
                ui.text(tl!("tap-here"))
                    .pos(0., mid_y)
                    .anchor(0.5, 0.5)
                    .size(0.5)
                    .color(Color::new(0., 0.2, 0., c.a))
                    .draw();
            };
            return;
        }

        ui.fill_rect(
            Rect::new(wf_x, mid_y - 0.001, wf_w, 0.002),
            Color::new(0.25, 0.25, 0.30, c.a),
        );

        let n = self.viz_samples.len();
        if n < 2 {
            return;
        }

        let max_val = self.viz_samples.iter().cloned().fold(0.0f32, f32::max).max(0.001);
        let max_points = 400;
        let step = (n as f32 / max_points as f32).max(1.0);

        let mut prev_x = wf_x;
        let mut prev_y = mid_y - (self.viz_samples[0] / max_val) * wf_h / 2.0;
        let mut i: f32 = step;
        while (i as usize) < n {
            let idx = i as usize;
            let x = wf_x + (idx as f32 / n as f32) * wf_w;
            let y = mid_y - (self.viz_samples[idx] / max_val) * wf_h / 2.0;
            let y = y.clamp(wf_y, wf_y + wf_h);
            let line_w = (x - prev_x).max(0.001);
            ui.fill_rect(
                Rect::new(prev_x, prev_y.min(y), line_w, (y - prev_y).abs().max(0.001)),
                Color::new(0.4, 0.2, 0.6, c.a),
            );
            prev_x = x;
            prev_y = y;
            i += step;
        }

        for (k, &edge) in self.viz_edges.iter().enumerate() {
            if k >= 2 {
                break;
            }
            let ex = wf_x + (edge as f32 / n as f32) * wf_w;
            let edge_color = if k == 0 {
                Color::new(1.0, 0.8, 0.2, 0.9 * c.a)
            } else {
                Color::new(0.3, 0.9, 0.3, 0.9 * c.a)
            };
            ui.fill_rect(
                Rect::new(ex - 0.002, wf_y, 0.004, wf_h),
                edge_color,
            );
            let time_ms = edge as f64 / self.viz_sr as f64 * 1000.0;
            let is_top = if k % 2 == 0 { true } else { false };
            let lx = if ex > wf_x + wf_w / 2.0 {
                ex - 0.15
            } else {
                ex + 0.01
            };
            ui.text(format!("{:.2}ms", time_ms))
                .pos(lx, if is_top { wf_y - aspect * 0.03 } else { wf_y + wf_h + aspect * 0.03 })
                .anchor(0.5, 0.5)
                .size(0.25)
                .color(edge_color)
                .draw();
        }
    }
}

fn create_recorder(
    buffer: Arc<Mutex<RingBuffer>>,
    position: Arc<AtomicU64>,
    sample_rate: Arc<AtomicU32>,
) -> Result<AudioRecorder> {
    #[cfg(target_os = "android")]
    {
        use sasa::backend::oboe::*;
        let mut recorder = AudioRecorder::new(OboeRecorderBackend::new(OboeSettings::default()))?;
        let tap_rec = TapRecorder::new(buffer, position, sample_rate);
        recorder.add_recorder(tap_rec)?;
        Ok(recorder)
    }
    #[cfg(not(target_os = "android"))]
    {
        use sasa::backend::cpal::*;
        let mut recorder = AudioRecorder::new(CpalRecorderBackend::new(CpalSettings::default()))?;
        let tap_rec = TapRecorder::new(buffer, position, sample_rate);
        recorder.add_recorder(tap_rec)?;
        Ok(recorder)
    }
}

fn high_pass(signal: &[f32], _sample_rate: f64, alpha: f64) -> Vec<f32> {
    let mut out = vec![0.0f32; signal.len()];
    let mut xn1 = 0.0f64;
    let mut yn1 = 0.0f64;
    for i in 0..signal.len() {
        let xn = signal[i] as f64;
        let yn = alpha * (yn1 + xn - xn1);
        out[i] = yn as f32;
        xn1 = xn;
        yn1 = yn;
    }
    out
}

fn average_filter(signal: &[f32]) -> Vec<f32> {
    let n = signal.len();
    if n == 0 {
        return vec![];
    }
    let avg: f64 = signal.iter().map(|&x| x as f64).sum::<f64>() / n as f64;
    let variance: f64 = signal.iter().map(|&x| {
        let d = x as f64 - avg;
        d * d
    }).sum::<f64>() / n as f64;
    let std_dev = variance.sqrt();
    let threshold = std_dev * 1.5;
    signal
        .iter()
        .map(|&x| {
            let d = (x as f64 - avg).abs();
            if d >= threshold { x } else { 0.0 }
        })
        .collect()
}

fn apply_envelope_and_scan(
    buffer: &[f32],
    sample_rate: f64,
    fast_buf: &mut Vec<f32>,
    slow_buf: &mut Vec<f32>,
    threshold_buf: &mut Vec<f32>,
) -> Vec<usize> {
    let n = buffer.len();
    let mut envelope = vec![0.0f32; n];
    let mut prev = 0.0f32;
    for i in 0..n {
        let input = buffer[i].abs();
        let output = if input > prev * 0.995 {
            input
        } else {
            prev * 0.995
        };
        prev = output;
        envelope[i] = output;
    }

    let mut events = Vec::new();
    let mut slow: f32 = 0.0;
    let mut fast: f32 = 0.0;
    let edge_threshold: f32 = 0.01;
    let slow_coeff: f32 = 0.01;
    let fast_coeff: f32 = 0.10;
    let mut low_threshold: f32 = edge_threshold;
    let mut armed = true;

    fast_buf.clear();
    slow_buf.clear();
    threshold_buf.clear();

    let skip = (sample_rate * 0.003) as usize;

    for i in 0..n {
        let level = envelope[i];
        slow += (level - slow) * slow_coeff;
        fast += (level - fast) * fast_coeff;
        slow_buf.push(slow);
        fast_buf.push(fast);

        if armed && i >= skip && fast > edge_threshold && fast > 2.0 * slow {
            events.push(i);
            armed = false;
            low_threshold = fast * 0.5;
        }
        threshold_buf.push(low_threshold);

        if fast < low_threshold {
            armed = true;
        }
    }

    events
}
