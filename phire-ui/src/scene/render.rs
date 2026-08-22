use super::fs_from_path;
use crate::get_data;
use anyhow::Result;
use macroquad::prelude::*;
use phire::{
    config::Mods,
    core::{AsyncRgbaReadback, Chart, HitSound, MSRenderTarget, ResourcePack, VideoWriter},
    ext::{poll_future, LocalTask},
    fs,
    scene::{BasicPlayer, GameMode, LoadingScene, NextScene, Scene},
    time::TimeManager,
    ui::Ui,
    ui::DRectButton,
};
use std::{cell::RefCell, collections::VecDeque, path::PathBuf, rc::Rc, time::Instant};
use sasa::{AudioClip, Frame};

const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;
const FPS: u32 = 60;
const VIDEO_CRF: i32 = 28;

pub struct RenderScene {
    output: PathBuf,
    target: Option<RenderTarget>,
    msaa: Option<MSRenderTarget>,
    load: LocalTask<Result<(LoadingScene, f64, Vec<Frame>)>>,
    scene: Option<Box<dyn Scene>>,
    writer: Option<VideoWriter>,
    audio: Vec<Frame>,
    audio_cursor: usize,
    frame: u64,
    total_frames: u64,
    pixels: Vec<u8>,
    readback: Option<AsyncRgbaReadback>,
    readback_frames: VecDeque<u64>,
    next_scene: Option<NextScene>,
    render_time: Rc<RefCell<f64>>,
    render_tm: TimeManager,
    started_at: Option<Instant>,
    last_progress_frame: u64,
    last_progress_at: Option<Instant>,
    render_fps: f64,
    cancel_button: DRectButton,
    cancelled: bool,
}

impl RenderScene {
    pub fn new(path: String, output: String) -> Self {
        let render_time = Rc::new(RefCell::new(0.0));
        let mut render_tm = TimeManager::manual(Box::new({
            let render_time = Rc::clone(&render_time);
            move || *render_time.borrow()
        }));
        render_tm.speed = 1.0;
        let load: LocalTask<Result<(LoadingScene, f64, Vec<Frame>)>> = Some(Box::pin(async move {
            let mut fs = fs_from_path(&path)?;
            let info = fs::load_info(fs.as_mut()).await?;
            let mut config = get_data().config.clone();
            config.mods.insert(Mods::AUTOPLAY);
            config.enter_animation = false;
            let volume_music = config.volume_music;
            let volume_sfx = config.volume_sfx;
            let (chart, format) = phire::scene::GameScene::load_chart(fs.as_mut(), &info, &config).await?;
            let (music, sample_rate) = AudioClip::decode(fs.load_file(&info.music).await?)?;
            let music = resample_audio(&music, sample_rate);
            let music_length = music.len() as f64 / 48_000.0;
            let before_time = if config.enter_animation { phire::scene::GameScene::BEFORE_DURATION } else { 0.0 };
            let play_end = config.play_end_time.unwrap_or(music_length).min(music_length);
            let duration = before_time + play_end / config.speed.max(f32::EPSILON) as f64 - config.play_start_time / config.speed.max(f32::EPSILON) as f64 + chart.offset + info.offset;
            let respack = ResourcePack::from_path(config.res_pack_path.as_ref()).await?;
            let audio = mix_audio(
                &chart,
                &respack,
                music,
                volume_music,
                volume_sfx,
                duration,
                config.play_start_time,
                chart.offset + info.offset,
                config.speed as f64,
                before_time,
            );
            config.volume_music = 0.0;
            config.volume_sfx = 0.0;
            let player = get_data().me.as_ref().map(|it| BasicPlayer {
                avatar: crate::client::UserManager::get_avatar(it.id).flatten(),
                id: it.id,
                rks: it.rks,
            });
            let loading = LoadingScene::new(Some((chart, format)), GameMode::Normal, info, &config, fs, player, None, None).await?;
            Ok((loading, duration, audio))
        }));
        Self {
            output: output.into(),
            target: None,
            msaa: None,
            load,
            scene: None,
            writer: None,
            audio: Vec::new(),
            audio_cursor: 0,
            frame: 0,
            total_frames: 0,
            pixels: Vec::new(),
            readback: None,
            readback_frames: VecDeque::new(),
            next_scene: None,
            render_time,
            render_tm,
            started_at: None,
            last_progress_frame: 0,
            last_progress_at: None,
            render_fps: 0.0,
            cancel_button: DRectButton::new(),
            cancelled: false,
        }
    }
}

fn resample_audio(frames: &[Frame], sample_rate: u32) -> Vec<Frame> {
    const OUTPUT_RATE: u32 = 48_000;
    if sample_rate == OUTPUT_RATE {
        return frames.to_vec();
    }
    let output_len = (frames.len() as u64 * OUTPUT_RATE as u64 / sample_rate as u64) as usize;
    (0..output_len)
        .map(|index| {
            let position = index as f64 * sample_rate as f64 / OUTPUT_RATE as f64;
            let left = position.floor() as usize;
            let fraction = (position - left as f64) as f32;
            let a = frames.get(left).copied().unwrap_or_default();
            let b = frames.get(left + 1).copied().unwrap_or(a);
            a.interpolate(&b, fraction)
        })
        .collect()
}

fn mix_audio(
    chart: &Chart,
    respack: &ResourcePack,
    music: Vec<Frame>,
    volume_music: f32,
    volume_sfx: f32,
    duration: f64,
    play_start_time: f64,
    chart_offset: f64,
    speed: f64,
    before_time: f64,
) -> Vec<Frame> {
    let sample_rate = 48_000usize;
    let output_len = (duration.max(0.) * sample_rate as f64).ceil() as usize;
    let mut output = vec![Frame::default(); output_len];
    let place = |output: &mut [Frame], clip: &[Frame], position: isize, volume: f32| {
        if volume == 0.0 {
            return;
        }
        for (index, source) in clip.iter().enumerate() {
            let target = position + index as isize;
            if target < 0 || target as usize >= output.len() {
                continue;
            }
            let target = &mut output[target as usize];
            target.0 += source.0 * volume;
            target.1 += source.1 * volume;
        }
    };

    let source_start = ((play_start_time - chart_offset.min(0.0)) * sample_rate as f64).ceil() as usize;
    let music_position = ((before_time) * sample_rate as f64).ceil() as isize;
    if source_start < music.len() {
        place(&mut output, &music[source_start..], music_position, volume_music);
    }

    let click = resample_audio(respack.sfx_click.frames(), respack.sfx_click.sample_rate());
    let drag = resample_audio(respack.sfx_drag.frames(), respack.sfx_drag.sample_rate());
    let flick = resample_audio(respack.sfx_flick.frames(), respack.sfx_flick.sample_rate());
    let custom: Vec<(String, Vec<Frame>)> = chart
        .hitsounds
        .iter()
        .map(|(name, clip)| (name.clone(), resample_audio(clip.frames(), clip.sample_rate())))
        .collect();

    for line in &chart.lines {
        for note in &line.notes {
            if note.fake {
                continue;
            }
            let clip = match &note.hitsound {
                HitSound::Click => Some(click.as_slice()),
                HitSound::Drag => Some(drag.as_slice()),
                HitSound::Flick => Some(flick.as_slice()),
                HitSound::Custom(name) => custom.iter().find(|(key, _)| key == name).map(|(_, clip)| clip.as_slice()),
                HitSound::None => None,
            };
            if let Some(clip) = clip {
                let speed_time_ratio = if speed.abs() < f64::EPSILON { 1.0 } else { 1.0 / speed };
                let position = ((before_time + chart_offset + note.time * speed_time_ratio - play_start_time * speed_time_ratio) * sample_rate as f64).round() as isize;
                place(&mut output, clip, position, volume_sfx);
            }
        }
    }
    for frame in &mut output {
        frame.0 = frame.0.clamp(-1.0, 1.0);
        frame.1 = frame.1.clamp(-1.0, 1.0);
    }
    output
}

impl Scene for RenderScene {
    fn touch(&mut self, _tm: &mut TimeManager, touch: &Touch) -> Result<bool> {
        if self.cancel_button.touch(touch, 0.0) {
            self.cancelled = true;
            self.next_scene = Some(NextScene::Pop);
            return Ok(true);
        }
        Ok(false)
    }

    fn enter(&mut self, _tm: &mut TimeManager, _target: Option<RenderTarget>) -> Result<()> {
        let msaa = MSRenderTarget::new((WIDTH, HEIGHT), 1);
        self.target = Some(msaa.output());
        self.readback = Some(AsyncRgbaReadback::new(WIDTH, HEIGHT));
        self.msaa = Some(msaa);
        Ok(())
    }

    fn update(&mut self, _tm: &mut TimeManager) -> Result<()> {
        if let Some(load) = self.load.as_mut() {
            if let Some(result) = poll_future(load.as_mut()) {
                self.load = None;
                match result {
                    Ok((scene, duration, music)) => {
                        self.total_frames = (duration * FPS as f64).ceil() as u64;
                        self.started_at = Some(Instant::now());
                        self.last_progress_at = self.started_at;
                        self.audio = music;
                        let writer = VideoWriter::new(self.output.to_string_lossy(), WIDTH as _, HEIGHT as _, FPS as _, VIDEO_CRF)?;
                        self.writer = Some(writer);
                        let mut scene: Box<dyn Scene> = Box::new(scene);
                        scene.enter(&mut self.render_tm, self.target.clone())?;
                        self.scene = Some(scene);
                    }
                    Err(error) => self.next_scene = Some(NextScene::PopWithResult(Box::new(error))),
                }
            }
        }
        if let Some(scene) = self.scene.as_mut() {
            let time = self.frame as f64 / FPS as f64;
            *self.render_time.borrow_mut() = time;
            self.render_tm.seek_to(time);
            scene.update(&mut self.render_tm)?;
            if let NextScene::Replace(mut replacement) = scene.next_scene(&mut self.render_tm) {
                replacement.enter(&mut self.render_tm, self.target.clone())?;
                self.scene = Some(replacement);
            }
        }
        Ok(())
    }

    fn render(&mut self, _tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        let Some(scene) = self.scene.as_mut() else {
            ui.text("Loading...").pos(0., 0.).size(0.5).draw();
            return Ok(());
        };
        let time = self.frame as f64 / FPS as f64;
        *self.render_time.borrow_mut() = time;
        self.render_tm.seek_to(time);
        scene.render(&mut self.render_tm, ui)?;
        if let Some(target) = &self.target {
            unsafe {
                get_internal_gl().flush();
            }
            let pixels = self.readback.as_mut().and_then(|readback| readback.read(target.clone()));
            self.readback_frames.push_back(self.frame);
            if let Some(writer) = self.writer.as_mut() {
                if let Some(pixels) = pixels {
                    self.pixels = pixels;
                    let frame = self.readback_frames.pop_front().unwrap_or(self.frame);
                    let audio_end = ((frame + 1) as usize * 48_000 / FPS as usize).min(self.audio.len());
                    if audio_end > self.audio_cursor {
                        writer.write_audio(&self.audio[self.audio_cursor..audio_end])?;
                        self.audio_cursor = audio_end;
                    }
                    writer.write_rgba(&self.pixels, WIDTH as i32 * 4, frame as i64)?;
                }
            }
        }
        set_camera(&ui.camera());
        self.frame += 1;
        if let Some(last_at) = self.last_progress_at {
            let elapsed = last_at.elapsed().as_secs_f64();
            if elapsed >= 0.25 {
                self.render_fps = (self.frame - self.last_progress_frame) as f64 / elapsed;
                self.last_progress_frame = self.frame;
                self.last_progress_at = Some(Instant::now());
            }
        }
        let progress = if self.total_frames == 0 { 0.0 } else { self.frame as f64 / self.total_frames as f64 };
        let remaining = if self.render_fps > 0.0 {
            (self.total_frames.saturating_sub(self.frame) as f64 / self.render_fps).round()
        } else {
            0.0
        };
        let progress_y = ui.top - 0.08;
        let cancel_rect = Rect::new(-0.92, ui.top - 0.16, 0.24, 0.10);
        self.cancel_button
            .render_text(ui, cancel_rect, 0.0, 1.0, ttl!("cancel"), 0.65, true);
        ui.text(format!("Rendering {:.1}%  {:.1} FPS  ETA {:.0}s", progress * 100.0, self.render_fps, remaining))
            .anchor(0.0, 0.5)
            .pos(-0.62, progress_y)
            .size(0.48)
            .draw();
        Ok(())
    }

    fn next_scene(&mut self, _tm: &mut TimeManager) -> NextScene {
        if let Some(next_scene) = self.next_scene.take() {
            if self.cancelled {
                self.writer.take();
                // let _ = std::fs::remove_file(&self.output);
            }
            return next_scene;
        }
        if self.total_frames > 0 && self.frame >= self.total_frames {
            if let Some(mut writer) = self.writer.take() {
                if let Some(readback) = self.readback.as_mut() {
                    for pixels in readback.drain() {
                        self.pixels = pixels;
                        let frame = self.readback_frames.pop_front().unwrap_or(self.frame);
                        let audio_end = ((frame + 1) as usize * 48_000 / FPS as usize).min(self.audio.len());
                        if audio_end > self.audio_cursor {
                            if let Err(error) = writer.write_audio(&self.audio[self.audio_cursor..audio_end]) {
                                return NextScene::PopWithResult(Box::new(anyhow::Error::from(error)));
                            }
                            self.audio_cursor = audio_end;
                        }
                        if let Err(error) = writer.write_rgba(&self.pixels, WIDTH as i32 * 4, frame as i64) {
                            return NextScene::PopWithResult(Box::new(anyhow::Error::from(error)));
                        }
                    }
                }
                if let Err(error) = writer.finish() {
                    return NextScene::PopWithResult(Box::new(anyhow::Error::from(error)));
                }
                phire::scene::finish_save_file(self.output.to_string_lossy().as_ref());
            }
            return NextScene::Pop;
        }
        NextScene::None
    }
}

use macroquad::window::get_internal_gl;
