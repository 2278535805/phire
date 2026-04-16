crate::tl_file!("parser");

use super::{BpmList, Effect, JudgeLine, JudgeLineKind, Matrix, Resource, UIElement, Vector};
use crate::{core::Object, fs::FileSystem, judge::JudgeStatus, ui::Ui};
use anyhow::{Context, Result};
use macroquad::prelude::*;
use rustc_hash::FxHashMap;
use sasa::AudioClip;
use std::{cell::RefCell};
use nalgebra::Rotation2;

#[derive(Default)]
pub struct ChartExtra {
    pub effects: Vec<Effect>,
    pub global_effects: Vec<Effect>,
    #[cfg(feature = "video")]
    pub videos: Vec<(super::Video, Option<super::VideoAttach>)>,
}

#[derive(Default)]
pub struct ChartSettings {
    pub pe_alpha_extension: bool,
}

pub type HitSoundMap = FxHashMap<String, AudioClip>;
const PROGRESS_BAR_COLOR: Color = Color::new(0.565, 0.565, 0.565, 1.0);

pub struct Chart {
    pub offset: f64,
    pub lines: Vec<JudgeLine>,
    pub bpm_list: RefCell<BpmList>,
    pub settings: ChartSettings,
    pub extra: ChartExtra,

    pub order: Vec<usize>,
    pub attach_ui: [Option<usize>; 7],
    pub hitsounds: HitSoundMap,
}

impl Chart {
    pub fn new(offset: f64, lines: Vec<JudgeLine>, bpm_list: BpmList, settings: ChartSettings, extra: ChartExtra, hitsounds: HitSoundMap) -> Self {
        let mut attach_ui = [None; 7];
        let mut order = (0..lines.len())
            .filter(|it| {
                if let Some(element) = lines[*it].attach_ui {
                    attach_ui[element as usize - 1] = Some(*it);
                    false
                } else {
                    true
                }
            })
            .collect::<Vec<_>>();
        order.sort_by_key(|it| (lines[*it].z_index, *it));
        Self {
            offset,
            lines,
            bpm_list: RefCell::new(bpm_list),
            settings,
            extra,

            order,
            attach_ui,
            hitsounds,
        }
    }

    #[inline]
    pub fn with_element<R>(&self, ui: &mut Ui, res: &Resource, element: UIElement, scale_point: Option<(f32, f32)>, rotation_point: Option<(f32, f32)>, f: impl FnOnce(&mut Ui, Color) -> R) -> R {
        let default_color = if matches!(element, UIElement::Bar) { PROGRESS_BAR_COLOR } else { WHITE };
        if let Some(id) = self.attach_ui[element as usize - 1] {
            let lines = &self.lines;
            let line = &lines[id];
            let object = &line.object;
            let translation = {
                let mut tr = line.fetch_pos(res, lines);
                tr.y *= -res.aspect_ratio;
                tr.x *= res.aspect_ratio;
                let sc = object.now_scale_wrt_point(scale_point.map_or_else(|| Vector::default(), |(x, y)| Vector::new(x, y)));
                let ro = 
                Object::new_rotation_wrt_point(Rotation2::new(-line.fetch_rot(lines).to_radians()), rotation_point.map_or_else(|| Vector::default(), |(x, y)| Vector::new(x, y)));
                Matrix::new_translation(&tr) * ro * sc
            };
            let mut color = line.color.now_opt().unwrap_or(default_color);
            color.a *= object.now_alpha().max(0.); 
            ui.with(translation, |ui| f(ui, color))
        } else {
            f(ui, default_color)
        }
    }

    pub async fn load_textures(&mut self, fs: &mut dyn FileSystem) -> Result<()> {
        for line in &mut self.lines {
            if let JudgeLineKind::Texture(tex, path) = &mut line.kind {
                *tex = image::load_from_memory(&fs.load_file(path).await.with_context(|| format!("failed to load illustration {path}"))?)?.into();
            }
        }
        Ok(())
    }

    pub fn reset(&mut self) {
        self.lines
            .iter_mut()
            .flat_map(|it| it.notes.iter_mut())
            .for_each(|note| {
                note.judge = JudgeStatus::NotJudged;
                note.protected = false;
                note.object.set_time(0.0);
            });
        for line in &mut self.lines {
            line.cache.reset(&mut line.notes);
        }
    }

    pub fn update(&mut self, res: &mut Resource) {
        for line in &mut self.lines {
            line.object.set_time(res.time);
        }
        // TODO optimize
        let trs = self.lines.iter().map(|it| it.now_transform(res, &self.lines)).collect::<Vec<_>>();
        let mut guard = self.bpm_list.borrow_mut();
        for (index, (line, tr)) in self.lines.iter_mut().zip(trs).enumerate() {
            line.update(res, tr, &mut guard, index);
        }
        drop(guard);
        for effect in &mut self.extra.effects {
            effect.update(res);
        }
        #[cfg(feature = "video")]
        for (video, _) in &mut self.extra.videos {
            if let Err(err) = video.update(res.time) {
                tracing::warn!("video error: {err:?}");
            }
        }
    }

    pub fn render(&self, ui: &mut Ui, res: &mut Resource) {
        res.apply_model_of(&Matrix::identity().append_nonuniform_scaling(&Vector::new(if res.config.flip_x() { -1. } else { 1. }, -1.)), |res| {
            #[cfg(feature = "video")]
            for (video, attach) in &self.extra.videos {
                if let Some(attach) = attach {
                    let line = &self.lines[attach.line];
                    let color = line.color.now_opt().unwrap_or(res.judge_line_color);
                    let mat = Rotation2::new(
                        self.lines[attach.line].fetch_rot(&self.lines).to_radians() * attach.rotation_factor).to_homogeneous()
                        .append_translation(&self.lines[attach.line].fetch_pos(res, &self.lines).component_mul(&Vector::new(attach.position_x_factor, attach.position_y_factor)));
                    res.apply_model_of(&mat.append_nonuniform_scaling(&Vector::new(1., -1.)), |res| {
                        video.render(res.time, res.aspect_ratio, color);
                    });
                } else {
                    res.apply_model_of(&Matrix::identity().append_nonuniform_scaling(&Vector::new(1., -1.)), |res| {
                        video.render(res.time, res.aspect_ratio, WHITE);
                    });
                }
            }

            let mut guard = self.bpm_list.borrow_mut();
            for id in &self.order {
                self.lines[*id].render(ui, res, &self.lines, &mut guard, &self.settings, *id);
            }
            drop(guard);
            res.note_buffer.borrow_mut().draw_all();
            if res.config.aggressive_note {
                res.note_pos_map.clear();
            }
            if res.config.sample_count > 1 {
                unsafe { get_internal_gl() }.flush();
                if let Some(target) = &res.chart_target {
                    target.blit();
                }
            }
        });
    }
}
