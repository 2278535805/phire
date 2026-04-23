use super::{MSRenderTarget, Matrix, Point, NOTE_WIDTH_RATIO_BASE};
use crate::{
    config::Config,
    core::tween::Tweenable,
    ext::{SafeTexture, create_audio_manger, nalgebra_to_glm},
    fs::FileSystem,
    info::ChartInfo,
    particle::{AtlasConfig, ColorCurve, Curve, Emitter, EmitterConfig, Interpolation, ParticleShape}
};
use anyhow::{bail, Context, Result};
use macroquad::prelude::*;
use miniquad::{gl::{GLuint, GL_LINEAR}, Texture, TextureWrap};
use sasa::{AudioClip, AudioManager, Sfx};
use serde::Deserialize;
use std::{cell::RefCell, collections::{BTreeMap, HashMap, VecDeque}, ops::DerefMut, path::Path, sync::atomic::AtomicU32};
use rand_pcg::{
    Pcg32,
    rand_core::SeedableRng
};
use rustc_hash::FxHashMap;

pub const MAX_SIZE: usize = 256; // needs tweaking
pub static DPI_VALUE: AtomicU32 = AtomicU32::new(250);
pub const BUFFER_SIZE: usize = 1024;
pub const RNG_SEED: u64 = 0x7a_61_6b_6f;

#[inline]
fn default_scale() -> f32 {
    1.
}

#[inline]
fn default_duration() -> f32 {
    0.5
}

#[inline]
fn default_perfect_fx() -> (f32, f32, f32, f32) {
    (1.0, 0.9, 0.65, 0.9)
}

#[inline]
fn default_good_fx() -> (f32, f32, f32, f32) {
    (0.70, 0.9, 1.0, 0.9)
}

#[inline]
fn default_perfect_line() -> (f32, f32, f32, f32) {
    (1.0, 1.0, 0.7, 1.0)
}

#[inline]
fn default_good_line() -> (f32, f32, f32, f32) {
    (0.65, 0.94, 1.0, 1.0)
}

#[inline]
fn default_tinted() -> bool {
    true
}

#[inline]
fn default_particle_count() -> usize {
    4
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResPackInfo {
    pub name: String,
    pub author: String,

    pub hit_fx: (u32, u32),
    #[serde(default = "default_duration")]
    pub hit_fx_duration: f32,
    #[serde(default = "default_scale")]
    pub hit_fx_scale: f32,
    #[serde(default)]
    pub hit_fx_rotate: bool,
    #[serde(default)]
    pub hide_particles: bool,
    #[serde(default)]
    pub circle_particles: bool,
    #[serde(default = "default_particle_count")]
    pub particle_count: usize,
    #[serde(default = "default_tinted")]
    pub hit_fx_tinted: bool,
    #[serde(default = "default_tinted")]
    pub line_tinted: bool,

    pub hold_atlas: (u32, u32),
    #[serde(rename = "holdAtlasMH")]
    pub hold_atlas_mh: (u32, u32),

    #[serde(default)]
    pub hold_keep_head: bool,
    #[serde(default)]
    pub hold_repeat: bool,
    #[serde(default)]
    pub hold_compact: bool,

    #[serde(default = "default_perfect_fx")]
    pub color_perfect_fx: (f32, f32, f32, f32),
    #[serde(default = "default_good_fx")]
    pub color_good_fx: (f32, f32, f32, f32),

    #[serde(default = "default_perfect_line")]
    pub color_perfect_line: (f32, f32, f32, f32),
    #[serde(default = "default_good_line")]
    pub color_good_line: (f32, f32, f32, f32),

    #[serde(default)]
    pub description: String,
}

impl ResPackInfo {
    pub fn fx_perfect(&self) -> Color {
        if self.hit_fx_tinted {
            Color::new(self.color_perfect_fx.0, self.color_perfect_fx.1, self.color_perfect_fx.2, self.color_perfect_fx.3)
        } else {
            WHITE
        }
    }

    pub fn fx_good(&self) -> Color {
        if self.hit_fx_tinted {
            Color::new(self.color_good_fx.0, self.color_good_fx.1, self.color_good_fx.2, self.color_good_fx.3)
        } else {
            WHITE
        }
    }

    pub fn line_perfect(&self) -> Color {
        if self.line_tinted {
            Color::new(self.color_perfect_line.0, self.color_perfect_line.1, self.color_perfect_line.2, self.color_perfect_line.3)
        } else {
            WHITE
        }
    }

    pub fn line_good(&self) -> Color {
        if self.line_tinted {
            Color::new(self.color_good_line.0, self.color_good_line.1, self.color_good_line.2, self.color_good_line.3)
        } else {
            WHITE
        }
    }
}

pub struct NoteStyle {
    pub click: SafeTexture,
    pub hold: SafeTexture,
    pub flick: SafeTexture,
    pub drag: SafeTexture,
    pub hold_body: Option<SafeTexture>,
    pub hold_atlas: (u32, u32),
}

impl NoteStyle {
    pub fn verify(&self) -> Result<()> {
        if (self.hold_atlas.0 + self.hold_atlas.1) as f32 >= self.hold.height() {
            bail!("Invalid atlas");
        }
        Ok(())
    }

    #[inline]
    fn to_uv(&self, t: u32) -> f32 {
        t as f32 / self.hold.height()
    }

    pub fn hold_ratio(&self) -> f32 {
        self.hold.height() / self.hold.width()
    }

    pub fn hold_head_rect(&self) -> Rect {
        let sy = self.to_uv(self.hold_atlas.1);
        Rect::new(0., 1. - sy, 1., sy)
    }

    pub fn hold_body_rect(&self) -> Rect {
        let sy = self.to_uv(self.hold_atlas.0);
        let ey = 1. - self.to_uv(self.hold_atlas.1);
        Rect::new(0., sy, 1., ey - sy)
    }

    pub fn hold_tail_rect(&self) -> Rect {
        let ey = self.to_uv(self.hold_atlas.0);
        Rect::new(0., 0., 1., ey)
    }
}

pub struct ResourcePack {
    pub info: ResPackInfo,
    pub note_style: NoteStyle,
    pub note_style_mh: NoteStyle,
    pub sfx_click: AudioClip,
    pub sfx_drag: AudioClip,
    pub sfx_flick: AudioClip,
    pub endings: [AudioClip; 8],
    pub hit_fx: SafeTexture,
}

impl ResourcePack {
    pub async fn from_path<T: AsRef<Path>>(path: Option<T>) -> Result<Self> {
        Self::load(
            if let Some(path) = path {
                crate::fs::fs_from_file(path.as_ref())?
            } else {
                crate::fs::fs_from_assets(format!("respack{}", std::path::MAIN_SEPARATOR))?
            }
            .deref_mut(),
        )
        .await
    }

    pub async fn load(fs: &mut dyn FileSystem) -> Result<Self> {
        macro_rules! load_tex {
            ($path:literal) => {
                SafeTexture::from(image::load_from_memory(&fs.load_file($path).await.with_context(|| format!("Missing {}", $path))?)?).with_filter(GL_LINEAR)
            };
        }
        let info: ResPackInfo = serde_yaml::from_str(&String::from_utf8(fs.load_file("info.yml").await.context("Missing info.yml")?)?)?;
        let mut note_style = NoteStyle {
            click: load_tex!("click.png"),
            hold: load_tex!("hold.png"),
            flick: load_tex!("flick.png"),
            drag: load_tex!("drag.png"),
            hold_body: None,
            hold_atlas: info.hold_atlas,
        };
        note_style.verify()?;
        let mut note_style_mh = NoteStyle {
            click: load_tex!("click_mh.png"),
            hold: load_tex!("hold_mh.png"),
            flick: load_tex!("flick_mh.png"),
            drag: load_tex!("drag_mh.png"),
            hold_body: None,
            hold_atlas: info.hold_atlas_mh,
        };
        note_style_mh.verify()?;
        if info.hold_repeat {
            fn get_body(style: &mut NoteStyle) {
                let pixels = style.hold.get_texture_data();
                let width = style.hold.width() as u16;
                let height = style.hold.height() as u16;
                let atlas = style.hold_atlas;
                let res = Texture2D::from_rgba8(
                    width,
                    height - atlas.0 as u16 - atlas.1 as u16,
                    &pixels.bytes[(atlas.0 as usize * width as usize * 4)..(pixels.bytes.len() - atlas.1 as usize * width as usize * 4)],
                );
                let context = unsafe { get_internal_gl() }.quad_context;
                res.raw_miniquad_texture_handle().set_wrap(context, TextureWrap::Repeat);
                style.hold_body = Some(res.into());
            }
            get_body(&mut note_style);
            get_body(&mut note_style_mh);
        }
        let hit_fx = image::load_from_memory(&fs.load_file("hit_fx.png").await.context("Missing hit_fx.png")?)?.into();

        macro_rules! load_clip {
            ($path:literal) => {
                if let Some(sfx) = fs.load_file(format!("{}.ogg", $path).as_str()).await.ok().map(|it| AudioClip::new(it)).transpose()? {
                    sfx
                } else if let Some(sfx) = fs.load_file(format!("{}.wav", $path).as_str()).await.ok().map(|it| AudioClip::new(it)).transpose()? {
                    sfx
                } else if let Some(sfx) = fs.load_file(format!("{}.mp3", $path).as_str()).await.ok().map(|it| AudioClip::new(it)).transpose()? {
                    sfx
                } else {
                    AudioClip::new(load_file(format!("{}.ogg", $path).as_str()).await?)?
                }
            };
        }

        macro_rules! load_ending {
            ($suffix:literal) => {
                if let Some(sfx) = fs.load_file(format!("ending{}.ogg", $suffix).as_str()).await.ok().map(|it| AudioClip::new(it)).transpose()? {
                    sfx
                } else if let Some(sfx) = fs.load_file(format!("ending{}.mp3", $suffix).as_str()).await.ok().map(|it| AudioClip::new(it)).transpose()? {
                    sfx
                } else if let Some(sfx) = fs.load_file(format!("ending.ogg").as_str()).await.ok().map(|it| AudioClip::new(it)).transpose()? {
                    sfx
                } else if let Some(sfx) = fs.load_file(format!("ending.mp3").as_str()).await.ok().map(|it| AudioClip::new(it)).transpose()? {
                    sfx
                } else if let Ok(file) = load_file(format!("ending{}.ogg", $suffix).as_str()).await {
                    AudioClip::new(file)?
                } else {
                    AudioClip::new(load_file(format!("ending.ogg").as_str()).await?)?
                }
            };
        }
        Ok(Self {
            info,
            note_style,
            note_style_mh,
            sfx_click: load_clip!("click"),
            sfx_drag: load_clip!("drag"),
            sfx_flick: load_clip!("flick"),
            endings: [
                load_ending!("_ap"),
                load_ending!("_fc"),
                load_ending!("_v"),
                load_ending!("_s"),
                load_ending!("_a"),
                load_ending!("_b"),
                load_ending!("_c"),
                load_ending!("")
                ],
            hit_fx,
        })
    }
}

pub struct ParticleEmitter {
    pub scale: f32,
    pub emitter: Emitter,
    pub emitter_square: Emitter,
    pub hide_particles: bool,
    pub particle_count: usize,
}

impl ParticleEmitter {
    pub fn new(res_pack: &ResourcePack, scale: f32, config: Option<Config>) -> Self {
        let colors_curve = {
            let start = WHITE;
            let mut mid = start;
            let mut end = start;
            mid.a *= 0.7;
            end.a = 0.;
            ColorCurve { start, mid, end }
        };
        let size_curve = Some(Curve {
            points: vec![
                (0.0, 1.0),
                (0.5, 0.9),
                (0.8, 0.8),
                (1.0, 0.7),
            ],
            interpolation: Interpolation::Linear,
            resolution: 10,
        });
        let config_default = Config::default();
        let config = config.unwrap_or(config_default);
        let emitter_config = EmitterConfig {
            max_particles: config.max_particles,
            local_coords: false,
            texture: Some(*res_pack.hit_fx),
            lifetime: res_pack.info.hit_fx_duration,
            lifetime_randomness: 0.0,
            initial_rotation_randomness: 0.0,
            initial_direction_spread: 0.0,
            initial_velocity: 0.0,
            atlas: Some(AtlasConfig::new(res_pack.info.hit_fx.0 as _, res_pack.info.hit_fx.1 as _, ..)),
            emitting: false,
            colors_curve,
            ..Default::default()
        };
        let shape = if res_pack.info.circle_particles {
            ParticleShape::Circle { subdivisions: 16 }
        } else {
            ParticleShape::Rectangle { aspect_ratio: 1.0 }
        };
        let emitter_square_config = EmitterConfig {
            max_particles: config.max_particles * res_pack.info.particle_count,
            rng: Some(Pcg32::seed_from_u64(RNG_SEED)),
            local_coords: false,
            lifetime: res_pack.info.hit_fx_duration,
            lifetime_randomness: 0.0,
            initial_direction_spread: 2. * std::f32::consts::PI,
            size_randomness: 0.3,
            emitting: false,
            initial_velocity: 2.0 * scale,
            initial_velocity_randomness: 0.3  * scale,
            linear_accel: |t| -(f32::tween(&1.6, &0.0, t.powi(4)).powi(2)),
            shape,
            colors_curve,
            size_curve,
            ..Default::default()
        };
        let mut res = Self {
            scale: res_pack.info.hit_fx_scale,
            emitter: Emitter::new(emitter_config),
            emitter_square: Emitter::new(emitter_square_config),
            hide_particles: res_pack.info.hide_particles,
            particle_count: res_pack.info.particle_count,
        };
        res.set_scale(scale);
        res
    }

    pub fn emit_at(&mut self, pt: Vec2, rotation: f32, color: Color) {
        self.emitter.config.initial_rotation = rotation;
        self.emitter.config.base_color = color;
        self.emitter.emit(pt, 1);
        if !self.hide_particles {
            self.emitter_square.config.base_color = color;
            self.emitter_square.emit(pt, self.particle_count);
        }
    }

    pub fn draw(&mut self, dt: f32) {
        self.emitter.draw(vec2(0., 0.), dt);
        self.emitter_square.draw(vec2(0., 0.), dt);
    }

    pub fn set_scale(&mut self, scale: f32) {
        self.emitter.config.size = self.scale * scale / 5.;
        self.emitter_square.config.size = self.scale * scale / 44.;
    }
}

#[derive(Default)]
pub struct NoteBuffer(BTreeMap<(i8, GLuint), Vec<(Vec<Vertex>, Vec<u16>)>>);
pub type SfxMap = HashMap<String, Sfx>;

impl NoteBuffer {
    pub fn push(&mut self, key: (i8, GLuint), vertices: [Vertex; 4]) {
        let meshes = self.0.entry(key).or_default();
        if meshes.last().map_or(true, |it| it.0.len() + 4 > MAX_SIZE * 4) {
            meshes.push(Default::default());
        }
        let last = meshes.last_mut().unwrap();
        let i = last.0.len() as u16;
        last.0.extend_from_slice(&vertices);
        last.1.extend_from_slice(&[i, i + 1, i + 2, i, i + 2, i + 3]);
    }

    pub fn draw_all(&mut self) {
        let mut gl = unsafe { get_internal_gl() };
        gl.flush();
        let gl = gl.quad_gl;
        gl.draw_mode(DrawMode::Triangles);
        for ((_, tex_id), meshes) in std::mem::take(&mut self.0).into_iter() {
            gl.texture(Some(Texture2D::from_miniquad_texture(unsafe { Texture::from_raw_id(tex_id, miniquad::TextureFormat::RGBA8) })));
            for mesh in meshes {
                gl.geometry(&mesh.0, &mesh.1);
            }
        }
    }
}

pub struct Resource {
    pub config: Config,
    pub info: ChartInfo,
    pub aspect_ratio: f32,
    pub dpi: u32,
    pub last_vp: (i32, i32, i32, i32),
    pub note_width: f32,

    pub time: f64,

    pub alpha: f32,
    pub judge_line_color: Color,

    pub camera: Camera2D,

    pub background: SafeTexture,
    pub illustration: SafeTexture,
    pub icons: [SafeTexture; 8],
    pub challenge_icons: [SafeTexture; 6],
    pub res_pack: ResourcePack,
    pub player: SafeTexture,
    pub icon_back: SafeTexture,
    pub icon_retry: SafeTexture,
    pub icon_resume: SafeTexture,
    pub icon_proceed: SafeTexture,

    pub emitter: ParticleEmitter,

    pub audio: AudioManager,
    pub music: AudioClip,
    pub track_length: f64,
    pub sfx_click: Sfx,
    pub sfx_drag: Sfx,
    pub sfx_flick: Sfx,
    pub extra_sfxs: SfxMap,
    pub frame_times: VecDeque<f64>, // frame interval time
    pub disable_hit_fx: bool,

    pub chart_target: Option<MSRenderTarget>,
    pub no_effect: bool,

    pub note_buffer: RefCell<NoteBuffer>,

    pub model_stack: Vec<Matrix>,
    #[cfg(feature = "play")]
    pub shake_play_mode_deque: VecDeque<(f64, f32)>, // time, acceleration
    #[cfg(feature = "play")]
    pub shake_play_paused: bool,

    particle_pos_map: FxHashMap<(i32, i32), VecDeque<f64>>,
    pub note_pos_map: FxHashMap<(i32, i32), u8>,
}

impl Resource {
    pub async fn load_icons() -> Result<[SafeTexture; 8]> {
        macro_rules! loads {
            ($($path:literal),*) => {
                [$(loads!(@detail $path)),*]
            };

            (@detail $path:literal) => {
                Texture2D::from_image(&load_image($path).await?).into()
            };
        }
        Ok(loads![
            "rank/phi.png",
            "rank/FC.png",
            "rank/V.png",
            "rank/S.png",
            "rank/A.png",
            "rank/B.png",
            "rank/C.png",
            "rank/F.png"
        ])
    }

    pub async fn load_challenge_icons() -> Result<[SafeTexture; 6]> {
        macro_rules! loads {
            ($($path:literal),*) => {
                [$(loads!(@detail $path)),*]
            };

            (@detail $path:literal) => {
                Texture2D::from_image(&load_image($path).await?).into()
            };
        }
        Ok(loads![
            "rank/white.png",
            "rank/green.png",
            "rank/blue.png",
            "rank/red.png",
            "rank/golden.png",
            "rank/rainbow.png"
        ])
    }

    pub async fn new(
        config: Config,
        info: ChartInfo,
        mut fs: Box<dyn FileSystem>,
        player: Option<SafeTexture>,
        background: SafeTexture,
        illustration: SafeTexture,
        has_no_effect: bool,
    ) -> Result<Self> {
        macro_rules! load_tex {
            ($path:literal) => {
                SafeTexture::from(Texture2D::from_image(&load_image($path).await?))
            };
        }
        let res_pack = ResourcePack::from_path(config.res_pack_path.as_ref())
            .await
            .context("Failed to load resource pack")?;
        let vec2_ratio = vec2(1.,-config.aspect_ratio.unwrap_or(info.aspect_ratio));
        let camera = Camera2D {
            target: vec2(0., 0.),
            zoom: vec2_ratio,
            ..Default::default()
        };

        let mut audio = create_audio_manger(&config)?;
        let music = AudioClip::new(fs.load_file(&info.music).await?)?;
        let music_length = music.length();
        let track_length = config.play_end_time.unwrap_or(music_length).min(music_length);
        let buffer_size = Some(BUFFER_SIZE);
        let sfx_click = audio.create_sfx(res_pack.sfx_click.clone(), buffer_size)?;
        let sfx_drag = audio.create_sfx(res_pack.sfx_drag.clone(), buffer_size)?;
        let sfx_flick = audio.create_sfx(res_pack.sfx_flick.clone(), buffer_size)?;
        let frame_times: VecDeque<f64> = VecDeque::new();

        let aspect_ratio = config.aspect_ratio.unwrap_or(info.aspect_ratio);
        let note_width = config.note_scale * NOTE_WIDTH_RATIO_BASE as f32;
        let note_scale = config.note_scale;

        let no_effect = !config.render_extra || has_no_effect;

        let emitter = ParticleEmitter::new(&res_pack, note_scale, Some(config.clone()));

        // aggressive 401 × 401 = 160,801
        let particle_pos_map = if config.aggressive_particle {
            FxHashMap::with_capacity_and_hasher(160801, Default::default())
        } else {
            FxHashMap::default()
        };
        let note_pos_map = if config.aggressive_note {
            FxHashMap::with_capacity_and_hasher(160801, Default::default())
        } else {
            FxHashMap::default()
        };

        macroquad::window::gl_set_drawcall_buffer_capacity(MAX_SIZE * 4, MAX_SIZE * 6);
        Ok(Self {
            config,
            info,
            aspect_ratio,
            dpi: DPI_VALUE.load(std::sync::atomic::Ordering::SeqCst),
            last_vp: (0, 0, 0, 0),
            note_width,

            time: 0.,

            alpha: 1.,
            judge_line_color: res_pack.info.line_perfect(),

            camera,

            background,
            illustration,
            icons: Self::load_icons().await?,
            challenge_icons: Self::load_challenge_icons().await?,
            res_pack,
            player: if let Some(player) = player { player } else { load_tex!("player.png") },
            icon_back: load_tex!("back.png"),
            icon_retry: load_tex!("retry.png"),
            icon_resume: load_tex!("resume.png"),
            icon_proceed: load_tex!("proceed.png"),

            emitter,

            audio,
            music,
            track_length,
            sfx_click,
            sfx_drag,
            sfx_flick,
            extra_sfxs: SfxMap::new(),
            frame_times,
            disable_hit_fx: false,

            chart_target: None,
            no_effect,

            note_buffer: RefCell::new(NoteBuffer::default()),

            model_stack: vec![Matrix::identity()],
            #[cfg(feature = "play")]
            shake_play_mode_deque: VecDeque::new(),
            #[cfg(feature = "play")]
            shake_play_paused: false,

            particle_pos_map,
            note_pos_map,
        })
    }

    pub fn reset(&mut self) {
        self.judge_line_color = self.res_pack.info.line_perfect();
        self.emitter.emitter_square.config.rng = Some(Pcg32::seed_from_u64(RNG_SEED));
    }

    pub fn emit_at_origin(&mut self, rotation: f32, color: Color) {
        if !self.config.particle {
            return;
        }
        let pt = self.world_to_screen(Point::default());

        if self.config.aggressive_particle {
            let roughly_pos = ((pt.x * 200.0).round() as i32, (pt.y * 200.0).round() as i32);
            let now = self.time;
            let duration = self.res_pack.info.hit_fx_duration as f64;

            let queue = self.particle_pos_map.entry(roughly_pos).or_default();

            while queue.front().is_some_and(|&t| now - t > duration) {
                queue.pop_front();
            }

            let recent_count = queue.iter().rev()
                .take_while(|&&t| (t - now).abs() < duration * 0.1)
                .count();

            if queue.len() < 10 || recent_count < 2 {
                queue.push_back(now);
            } else {
                return;
            }
        }

        self.emitter.emit_at(
            vec2(if self.config.flip_x() { -pt.x } else { pt.x }, -pt.y),
            if self.res_pack.info.hit_fx_rotate { rotation.to_radians() } else { 0. },
            color,
        );
    }

    pub fn update_size(&mut self, vp: (i32, i32, i32, i32)) -> bool {
        if self.last_vp == vp {
            return false;
        }
        self.last_vp = vp;
        let vp = if self.config.low_resolution_mode {
            (vp.0 / 2, vp.1 / 2, vp.2 / 2, vp.3 / 2)
        } else {
            vp
        };
        if !self.no_effect || self.config.sample_count != 1 || self.config.low_resolution_mode {
            self.chart_target = Some(MSRenderTarget::new((vp.2 as u32, vp.3 as u32), self.config.sample_count));
        }
        fn viewport(aspect_ratio: f32, (x, y, w, h): (i32, i32, i32, i32)) -> (i32, i32, i32, i32) {
            let w = w as f32;
            let h = h as f32;
            let (rw, rh) = {
                let ew = h * aspect_ratio;
                if ew > w {
                    let eh = w / aspect_ratio;
                    (w, eh)
                } else {
                    (ew, h)
                }
            };
            (x + ((w - rw) / 2.).round() as i32, y + ((h - rh) / 2.).round() as i32, rw as i32, rh as i32)
        }
        let aspect_ratio = self.config.aspect_ratio.unwrap_or(self.info.aspect_ratio);
        if self.info.force_aspect_ratio {
            self.aspect_ratio = aspect_ratio;
            self.camera.viewport = Some(viewport(aspect_ratio, vp));
        } else {
            self.aspect_ratio = aspect_ratio.min(vp.2 as f32 / vp.3 as f32);
            self.camera.zoom.y = -self.aspect_ratio;
            self.camera.viewport = Some(viewport(self.aspect_ratio, vp));
        };
        true
    }

    pub fn world_to_screen(&self, pt: Point) -> Point {
        self.model_stack.last().unwrap().transform_point(&pt)
    }

    pub fn screen_to_world(&self, pt: Point) -> Point {
        self.model_stack.last().unwrap().try_inverse().unwrap().transform_point(&pt)
    }

    #[inline]
    pub fn with_model(&mut self, model: Matrix, f: impl FnOnce(&mut Self)) {
        let model = self.model_stack.last().unwrap() * model;
        self.model_stack.push(model);
        f(self);
        self.model_stack.pop();
    }

    #[inline]
    pub fn apply_model(&mut self, f: impl FnOnce(&mut Self)) {
        self.apply_model_of(&self.model_stack.last().unwrap().clone(), f);
    }

    #[inline]
    pub fn apply_model_of(&mut self, mat: &Matrix, f: impl FnOnce(&mut Self)) {
        unsafe { get_internal_gl() }.quad_gl.push_model_matrix(nalgebra_to_glm(mat));
        f(self);
        unsafe { get_internal_gl() }.quad_gl.pop_model_matrix();
    }
}
