use super::{draw_background, ending::RecordUpdateState, game::GameMode, GameScene, NextScene, Scene};
use crate::{
    config::Config,
    core::{Chart, Resource},
    ext::{draw_illustration, draw_parallelogram, draw_text_aligned, draw_text_aligned_opt, draw_text_aligned_opt_width, poll_future, LocalTask, SafeTexture, BLACK_TEXTURE},
    fs::FileSystem,
    info::{ChartFormat, ChartInfo},
    judge::Judge,
    task::Task,
    time::TimeManager,
    ui::Ui,
};
use ::rand::{rng, seq::IndexedRandom};
use anyhow::{Context, Result};
use macroquad::prelude::*;
use regex::Regex;
use std::sync::Arc;
use tracing::warn;

const BEFORE_TIME: f64 = 1.;
const TRANSITION_TIME: f64 = 1.4;
const WAIT_TIME: f64 = 0.;

#[derive(Clone)]
pub struct UploadFn {
    pub refresh: Arc<dyn Fn() -> Task<()> + Send + Sync>,
    pub upload: Arc<dyn Fn(Vec<u8>) -> Task<Result<RecordUpdateState>>>,
}
pub type UpdateFn = Box<dyn FnMut(f64, &mut Resource, &mut Judge)>;

pub struct BasicPlayer {
    pub avatar: Option<SafeTexture>,
    pub id: i32,
    pub rks: f32,
}

pub struct LoadingScene {
    info: ChartInfo,
    config: Config,
    background: SafeTexture,
    illustration: SafeTexture,
    pub load_task: LocalTask<Result<GameScene>>,
    next_scene: Option<NextScene>,
    finish_time: f64,
    target: Option<RenderTarget>,
    charter: String,
}

impl LoadingScene {
    pub const TOTAL_TIME: f64 = BEFORE_TIME + TRANSITION_TIME + WAIT_TIME;

    pub async fn load_background(fs: &mut Box<dyn FileSystem>, config: &Config, path: &str) -> Result<(Texture2D, Texture2D)> {
        let image = image::load_from_memory(&fs.load_file(path).await?).context("Failed to decode image")?;
        let (w, h) = (image.width(), image.height());
        let size = w as usize * h as usize;

        let mut blurred_rgb = image.to_rgb8();
        let mut vec = unsafe { Vec::from_raw_parts(std::mem::transmute(blurred_rgb.as_mut_ptr()), size, size) };
        fastblur::gaussian_blur(&mut vec, w as _, h as _, config.bg_blurriness);
        std::mem::forget(vec);
        let mut blurred = Vec::with_capacity(size * 4);
        for input in blurred_rgb.chunks_exact(3) {
            //blurred.extend_from_slice(input);
            blurred.push(input[0]);
            blurred.push(input[1]);
            blurred.push(input[2]);
            blurred.push(255);
        }
        Ok((
            Texture2D::from_rgba8(w as _, h as _, &image.into_rgba8()),
            Texture2D::from_image(&Image {
                width: w as _,
                height: h as _,
                bytes: blurred,
            }),
        ))
    }

    pub async fn new(
        preload_chart: Option<(Chart, ChartFormat)>,
        mode: GameMode,
        mut info: ChartInfo,
        config: &Config,
        mut fs: Box<dyn FileSystem>,
        player: Option<BasicPlayer>,
        upload_fn: Option<UploadFn>,
        update_fn: Option<UpdateFn>,
    ) -> Result<Self> {
        let background = match Self::load_background(&mut fs, config, &info.illustration).await {
            Ok((ill, bg)) => Some((ill, bg)),
            Err(err) => {
                warn!("failed to load background: {err:?}");
                None
            }
        };
        let (illustration, background): (SafeTexture, SafeTexture) = background
            .map(|(ill, back)| (ill.into(), back.into()))
            .unwrap_or_else(|| (BLACK_TEXTURE.clone(), BLACK_TEXTURE.clone()));
        if info.tip.is_none() {
            let tips_file = load_file("tips.txt".to_string().as_str()).await?;
            let tips = String::from_utf8_lossy(&tips_file)
                .lines()
                .map(|line| line.to_string())
                .collect::<Vec<_>>();

            info.tip = Some(tips.choose(&mut rng()).unwrap().to_owned());
        }
        let future = Box::pin(GameScene::new(preload_chart, mode, info.clone(), config.clone(), fs, player, background.clone(), illustration.clone(), upload_fn, update_fn));
        let charter = Regex::new(r"\[!:[0-9]+:([^:]*)\]").unwrap().replace_all(&info.charter, "$1").to_string();

        Ok(Self {
            info,
            config: config.clone(),
            background,
            illustration,
            load_task: Some(future),
            next_scene: None,
            finish_time: f64::INFINITY,
            target: None,
            charter,
        })
    }
}

impl Scene for LoadingScene {
    fn enter(&mut self, _tm: &mut TimeManager, target: Option<RenderTarget>) -> Result<()> {
        self.target = target;
        Ok(())
    }

    fn update(&mut self, tm: &mut TimeManager) -> Result<()> {
        if let Some(future) = self.load_task.as_mut() {
            loop {
                match poll_future(future.as_mut()) {
                    None => {
                        if self.target.is_none() {
                            break;
                        }
                        std::thread::yield_now();
                    }
                    Some(game_scene) => {
                        self.load_task = None;
                        self.next_scene =
                            Some(game_scene.map_or_else(|e| NextScene::PopWithResult(Box::new(e)), |it| NextScene::Replace(Box::new(it))));
                        self.finish_time = tm.now() + BEFORE_TIME;
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    fn pause(&mut self, tm: &mut TimeManager) -> Result<()> {
        tm.pause();
        Ok(())
    }

    fn resume(&mut self, tm: &mut TimeManager) -> Result<()> {
        tm.resume();
        Ok(())
    }

    fn render(&mut self, tm: &mut TimeManager, ui: &mut Ui) -> Result<()> {
        let cam = ui.camera();
        let asp = cam.zoom.y;
        let top = 1. / asp;
        let now = tm.now();
        let intern = unsafe { get_internal_gl() };
        let gl = intern.quad_gl;
        set_camera(&Camera2D {
            zoom: vec2(1., asp),
            render_target: self.target.clone(),
            ..Default::default()
        });
        if self.config.render_bg {
            draw_background(&self.background, self.config.render_bg_dim);
        }
        let dx = if now > self.finish_time {
            let p = ((now - self.finish_time) / TRANSITION_TIME).min(1.);
            p.powi(2) * 3. + p.powi(5) * 11.
        } else {
            0.
        };
        if dx != 0. {
            gl.push_model_matrix(Mat4::from_translation(vec3(dx as f32, 0., 0.)));
        }
        let vo = -top / 10.;
        let voi = -top / 8.5;
        let r = draw_illustration(&self.illustration, 0.380, voi, 1.03, 1.0, WHITE, false);
        let h = r.h / 3.55;
        let main: Rect = Rect::new(-0.87, vo - h / 2. - top / 10., 0.768, h);
        draw_parallelogram(main, None, Color::new(0., 0., 0., 0.6), false);
        let p1 = (main.x + main.w * 0.085, main.y + main.h * 0.35 + 0.025);
        let p2 = (main.x + main.w * 0.09, main.y + main.h * 0.74 - 0.0125);

        draw_text_aligned_opt(ui, &self.info.name, p1.0, p1.1, (0., 1.0), 0.73, WHITE, main.w * 0.65, main.h * 0.5);
        draw_text_aligned_opt(ui, &self.info.composer, p2.0, p2.1, (0., 0.0), 0.363, WHITE, main.w * 0.60, main.h * 0.25);

        let ext = 0.04;
        let sub = Rect::new(main.x + main.w * 0.724, main.y - main.h * ext, main.w * 0.25, main.h * (1. + ext * 2.));
        let mut ct = sub.center();
        ct.x += sub.w * 0.01;
        ct.y += sub.h * 0.05;
        draw_parallelogram(sub, None, WHITE, true);
        //draw_text_aligned(ui, &(self.info.difficulty as u32).to_string(), ct.x, ct.y + sub.h * 0.05, (0.5, 1.), 0.88, BLACK);
        if !self.config.difficulty.is_empty() {
            draw_text_aligned_opt(ui, &self.config.difficulty, ct.x, ct.y + sub.h * 0.05, (0.5, 1.), 0.90, BLACK, main.w * 0.18, main.h * 0.6);
        } else {
            let first_str = Regex::new(r"[0-9?]+").unwrap();
            let last_str = Regex::new(r"[0-9?.]+").unwrap();
            draw_text_aligned_opt_width(ui, self.info.level
                .split_whitespace()
                .next_back()
                //.and_then(|word| word.get(3..))
                .and_then(|word| { first_str.find(word).map(|m| &word[m.start()..]) })
                .and_then(|word| { last_str.find(word).map(|m| &word[..m.end()]) })
                //.unwrap_or_default()
                .unwrap_or(
                    //self.info.level.split_whitespace().rev().nth(0).and_then(|word| word.find('.').map(|pos| &word[(pos + 1)..])).unwrap_or("?")
                    "?"
                ),
                ct.x, ct.y + sub.h * 0.05, (0.5, 1.), 0.90, BLACK, main.w * 0.18
            );
        }
        //难度
        draw_text_aligned_opt_width(ui, self.info.level
            .split_whitespace()
            .next()
            .unwrap_or("?")
            , ct.x, ct.y + sub.h * 0.09, (0.5, 0.), 0.30, BLACK, main.w * 0.16
        );
        let w = 0.031;
        let h = 0.030;
        let (text_chart, text_illustration) = if self.config.chinese {("谱师", "画师")} else {("Chart", "Illustration")};
        let t = draw_text_aligned(ui, text_chart, main.x + main.w / 6.1, main.y + main.h * 1.32, (0., 0.), 0.253, WHITE);
        let t = draw_text_aligned_opt_width(ui, &self.charter, t.x, t.y + top / 22., (0., 0.), 0.415, WHITE, 0.58);
        let t = draw_text_aligned(ui, text_illustration, t.x - w, t.y + t.h + h, (0., 0.), 0.253, WHITE);
        draw_text_aligned_opt_width(ui, &self.info.illustrator, t.x - 0.002, t.y + top / 22., (0., 0.), 0.415, WHITE, 0.58);
        let text_tip = self.info.tip.as_ref().unwrap();
        draw_text_aligned_opt_width(ui, text_tip, -0.895, top * 0.88, (0., 1.), 0.47, WHITE, 1.55);
        let text_loading = if self.config.chinese {"加载中..."} else {"Loading..."};
        let t = draw_text_aligned(ui, text_loading, 0.865, top * 0.865, (1., 1.), 0.41, WHITE);
        let we = 0.19;
        let he = 0.35;
        let r = Rect::new(t.x - t.w * we, t.y - t.h * he, t.w * (1. + we * 2.2), t.h * (1. + he * 2.2));

        let p = 0.6;
        let s = 0.2;
        let t = ((now - 0.3).max(0.) % (p * 2. + s)) / p;
        let st = (t - 1.).clamp(0., 1.).powi(3) as f32;
        let en = 1. - (1. - t.min(1.)).powi(3) as f32;

        let mut r = Rect::new(r.x + r.w * st, r.y, r.w * (en - st), r.h);
        ui.fill_rect(r, WHITE);
        r.x += dx as f32;
        ui.scissor(r, |ui| {
            draw_text_aligned(ui, text_loading, 0.865, top * 0.865, (1., 1.), 0.41, BLACK);
        });

        if dx != 0. {
            gl.pop_model_matrix();
        }
        Ok(())
    }

    fn next_scene(&mut self, tm: &mut TimeManager) -> NextScene {
        if matches!(self.next_scene, Some(NextScene::PopWithResult(_))) {
            return self.next_scene.take().unwrap();
        }
        if tm.now() > self.finish_time + TRANSITION_TIME + WAIT_TIME || !self.config.enter_animation {
            if let Some(scene) = self.next_scene.take() {
                return scene;
            }
        }
        NextScene::None
    }
}
