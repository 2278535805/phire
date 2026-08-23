phire::tl_file!("song");

use super::fs_from_path;
use crate::get_data;
use anyhow::Result;
use macroquad::prelude::*;
use phire::{
    config::Mods,
    core::{read_render_target_rgba8, Chart, HitSound, MSRenderTarget, ResourcePack, VideoWriter},
    ext::{poll_future, LocalTask},
    fs,
    scene::{BasicPlayer, GameMode, LoadingScene, NextScene, Scene},
    time::TimeManager,
    ui::{DRectButton, InlineInputBox, Ui},
};
use phire::ext::{semi_black, RectExt};
use std::{cell::RefCell, path::PathBuf, rc::Rc, time::Instant};
use sasa::{AudioClip, Frame};

fn dialog_rect() -> Rect {
    let hw = 0.45;
    let hh = 0.45;
    Rect::new(-hw, -hh, hw * 2., hh * 2.)
}

pub struct RenderConfigDialog {
    show: bool,
    resolution: String,
    fps: String,
    crf: String,
    resolution_button: DRectButton,
    fps_button: DRectButton,
    crf_button: DRectButton,
    resolution_input: InlineInputBox,
    fps_input: InlineInputBox,
    crf_input: InlineInputBox,
    cancel_button: DRectButton,
    confirm_button: DRectButton,
    pub result: Option<Option<((u32, u32), u32, i32)>>,
}

impl RenderConfigDialog {
    pub fn new() -> Self {
        Self {
            show: false,
            resolution: "1280x720".to_owned(),
            fps: "60".to_owned(),
            crf: "24".to_owned(),
            resolution_button: DRectButton::new(),
            fps_button: DRectButton::new(),
            crf_button: DRectButton::new(),
            resolution_input: InlineInputBox::new(),
            fps_input: InlineInputBox::new(),
            crf_input: InlineInputBox::new(),
            cancel_button: DRectButton::new(),
            confirm_button: DRectButton::new(),
            result: None,
        }
    }

    pub fn show(&mut self) {
        self.show = true;
        self.result = None;
    }

    pub fn touch(&mut self, touch: &Touch, t: f32) -> bool {
        if !self.show { return false; }
        for (input, value) in [
            (&mut self.resolution_input, &mut self.resolution),
            (&mut self.fps_input, &mut self.fps),
            (&mut self.crf_input, &mut self.crf),
        ] {
            if input.is_active() {
                if input.touch(touch) { *value = input.confirm(); }
                return true;
            }
        }
        if self.resolution_button.touch(touch, t) {
            self.resolution_input.activate(&self.resolution, false, false);
        } else if self.fps_button.touch(touch, t) {
            self.fps_input.activate(&self.fps, false, false);
        } else if self.crf_button.touch(touch, t) {
            self.crf_input.activate(&self.crf, false, false);
        } else if self.cancel_button.touch(touch, t) {
            self.show = false;
            self.result = Some(None);
        } else if self.confirm_button.touch(touch, t) {
            let resolution = self.resolution.split_once('x').or_else(|| self.resolution.split_once('X')).and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?)));
            let fps = self.fps.parse::<u32>().ok();
            let crf = self.crf.parse::<i32>().ok();
            if let (Some((w, h)), Some(fps), Some(crf)) = (resolution, fps, crf) {
                if w > 0 && h > 0 && w % 2 == 0 && h % 2 == 0 && (1..=240).contains(&fps) && (0..=51).contains(&crf) {
                    self.show = false;
                    self.result = Some(Some(((w, h), fps, crf)));
                }
            }
        }
        true
    }

    pub fn update(&mut self) {
        if self.resolution_input.is_active() { self.resolution_input.update(); }
        if self.fps_input.is_active() { self.fps_input.update(); }
        if self.crf_input.is_active() { self.crf_input.update(); }
    }

    pub fn render(&mut self, ui: &mut Ui, t: f32) {
        if !self.show { return; }
        ui.fill_rect(ui.screen_rect(), semi_black(0.75));
        let wr = dialog_rect().nonuniform_feather(0.0, -0.02);
        ui.fill_path(&wr.rounded(0.02), ui.background());
        ui.text(tl!("render-config")).pos(wr.x + 0.04, wr.y + 0.035).size(0.9).draw();

        let mut r = Rect::new(wr.x + 0.04, wr.y + 0.15, wr.w - 0.08, 0.1);

        ui.text(tl!("render-resolution")).pos(r.x, r.y).size(0.55).draw();
        r.y += 0.06;
        if self.resolution_input.is_active() { self.resolution_input.render(ui, r, 1.0, "1280x720"); }
        else { self.resolution_button.render_input(ui, r, t, 1.0, &self.resolution, "1280x720", 0.55); }
        r.y += 0.13;

        ui.text(tl!("render-fps")).pos(r.x, r.y).size(0.55).draw();
        r.y += 0.06;
        if self.fps_input.is_active() { self.fps_input.render(ui, r, 1.0, "60"); }
        else { self.fps_button.render_input(ui, r, t, 1.0, &self.fps, "60", 0.55); }
        r.y += 0.13;

        ui.text(tl!("render-quality")).pos(r.x, r.y).size(0.55).draw();
        r.y += 0.06;
        if self.crf_input.is_active() { self.crf_input.render(ui, r, 1.0, "24"); }
        else { self.crf_button.render_input(ui, r, t, 1.0, &self.crf, "24", 0.55); }
        
        let pad = 0.04;
        let bw = (wr.w - pad * 3.0) / 2.0;
        let mut r = Rect::new(wr.x + pad, wr.bottom() - 0.135, bw, 0.09);
        self.cancel_button.render_text(ui, r, t, 1.0, ttl!("cancel"), 0.55, true);
        r.x += bw + pad;
        self.confirm_button.render_text(ui, r, t, 1.0, ttl!("confirm"), 0.55, true);
    }
}

pub struct RenderScene {
    width: u32,
    height: u32,
    fps: u32,
    crf: i32,
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
    pub fn new(path: String, output: String, resolution: (u32, u32), fps: u32, crf: i32) -> Self {
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
            width: resolution.0,
            height: resolution.1,
            fps,
            crf,
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
        let msaa = MSRenderTarget::new((self.width, self.height), 1);
        self.target = Some(msaa.output());
        self.msaa = Some(msaa);
        Ok(())
    }

    fn update(&mut self, _tm: &mut TimeManager) -> Result<()> {
        if let Some(load) = self.load.as_mut() {
            if let Some(result) = poll_future(load.as_mut()) {
                self.load = None;
                match result {
                    Ok((scene, duration, music)) => {
                        self.total_frames = (duration * self.fps as f64).ceil() as u64;
                        self.started_at = Some(Instant::now());
                        self.last_progress_at = self.started_at;
                        self.audio = music;
                        let writer = VideoWriter::new(
                            self.output.to_string_lossy(),
                            self.width as _,
                            self.height as _,
                            self.fps as _,
                            self.crf,
                        )?;
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
            let time = self.frame as f64 / self.fps as f64;
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
            ui.text("Loading").pos(0., 0.).anchor(0.5, 0.5).size(0.5).draw();
            return Ok(());
        };
        let time = self.frame as f64 / self.fps as f64;
        *self.render_time.borrow_mut() = time;
        self.render_tm.seek_to(time);
        {
            let mut capture_ui = Ui::new(ui.text_painter, Some((0, 0, self.width as i32, self.height as i32)));
            scene.render(&mut self.render_tm, &mut capture_ui)?;
        }
        if let Some(target) = &self.target {
            unsafe {
                get_internal_gl().flush();
            }
            read_render_target_rgba8(target.clone(), (self.width, self.height), &mut self.pixels);
            if let Some(writer) = self.writer.as_mut() {
                let audio_end = ((self.frame + 1) as usize * 48_000 / self.fps as usize).min(self.audio.len());
                if audio_end > self.audio_cursor {
                    writer.write_audio(&self.audio[self.audio_cursor..audio_end])?;
                    self.audio_cursor = audio_end;
                }
                writer.write_rgba(&self.pixels, self.width as i32 * 4, self.frame as i64)?;
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
        let progress_y = ui.top - 0.11;
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
            if let Some(writer) = self.writer.take() {
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
