use super::{Anim, Resource, Tweenable};
use crate::ext::{get_viewport, screen_aspect};
use anyhow::{anyhow, bail, Result};
use macroquad::prelude::*;
use macroquad::miniquad::UniformType;
use once_cell::sync::Lazy;
use phf::phf_map;
use regex::Regex;
use std::{collections::HashSet, ops::Range};

static SHADERS: phf::Map<&'static str, &'static str> = phf_map! {
    "chromatic" => include_str!("shaders/chromatic.glsl"),
    "circleBlur" => include_str!("shaders/circle_blur.glsl"),
    "fisheye" => include_str!("shaders/fisheye.glsl"),
    "glitch" => include_str!("shaders/glitch.glsl"),
    "grayscale" => include_str!("shaders/grayscale.glsl"),
    "noise" => include_str!("shaders/noise.glsl"),
    "pixel" => include_str!("shaders/pixel.glsl"),
    "radialBlur" => include_str!("shaders/radial_blur.glsl"),
    "shockwave" => include_str!("shaders/shockwave.glsl"),
    "vignette" => include_str!("shaders/vignette.glsl"),
};

static RPE_SHADERS: phf::Map<&'static str, &'static str> = phf_map! {
    "bokeh_pr" => include_str!("shaders/rpe/bokeh_pr.glsl"),
    "burn_pr" => include_str!("shaders/rpe/burn_pr.glsl"),
    "camera_pr" => include_str!("shaders/rpe/camera_pr.glsl"),
    "color_shift_pr" => include_str!("shaders/rpe/color_shift_pr.glsl"),
    "distortion_pr" => include_str!("shaders/rpe/distortion_pr.glsl"),
    "emboss_pr" => include_str!("shaders/rpe/emboss_pr.glsl"),
    "flare_pr" => include_str!("shaders/rpe/flare_pr.glsl"),
    "flip_pr" => include_str!("shaders/rpe/flip_pr.glsl"),
    "flowing_pr" => include_str!("shaders/rpe/flowing_pr.glsl"),
    "fog_pr" => include_str!("shaders/rpe/fog_pr.glsl"),
    "glow_effect_pr" => include_str!("shaders/rpe/glow_effect_pr.glsl"),
    "heat_distortion_pr" => include_str!("shaders/rpe/heat_distortion_pr.glsl"),
    "hologram_pr" => include_str!("shaders/rpe/hologram_pr.glsl"),
    "image_noise_pr" => include_str!("shaders/rpe/image_noise_pr.glsl"),
    "kaleidoscope_pr" => include_str!("shaders/rpe/kaleidoscope_pr.glsl"),
    "lightning_pr" => include_str!("shaders/rpe/lightning_pr.glsl"),
    "liquid_pr" => include_str!("shaders/rpe/liquid_pr.glsl"),
    "neon_pr" => include_str!("shaders/rpe/neon_pr.glsl"),
    "night_vision_pr" => include_str!("shaders/rpe/night_vision_pr.glsl"),
    "oil_painting_pr" => include_str!("shaders/rpe/oil_painting_pr.glsl"),
    "old_tv_pr" => include_str!("shaders/rpe/old_tv_pr.glsl"),
    "rain_pr" => include_str!("shaders/rpe/rain_pr.glsl"),
    "rainbow_pr" => include_str!("shaders/rpe/rainbow_pr.glsl"),
    "rays_pr" => include_str!("shaders/rpe/rays_pr.glsl"),
    "scanline_pr" => include_str!("shaders/rpe/scanline_pr.glsl"),
    "snow_pr" => include_str!("shaders/rpe/snow_pr.glsl"),
    "sobel_pr" => include_str!("shaders/rpe/sobel_pr.glsl"),
    "two_tone_pr" => include_str!("shaders/rpe/two_tone_pr.glsl"),
    "underwater_pr" => include_str!("shaders/rpe/underwater_pr.glsl"),
    "wave_pr" => include_str!("shaders/rpe/wave_pr.glsl"),
};

pub trait UniformValue: Clone + Default {
    const UNIFORM_TYPE: UniformType;
}

impl UniformValue for f32 {
    const UNIFORM_TYPE: UniformType = UniformType::Float1;
}

impl UniformValue for Vec2 {
    const UNIFORM_TYPE: UniformType = UniformType::Float2;
}

impl UniformValue for Color {
    const UNIFORM_TYPE: UniformType = UniformType::Float4;
}

pub trait Uniform {
    fn uniform_pair(&self) -> (String, UniformType);
    fn set_time(&mut self, t: f64);
    fn apply(&self, material: &Material);
}

impl<T: UniformValue> Uniform for (String, T) {
    fn uniform_pair(&self) -> (String, UniformType) {
        (self.0.clone(), T::UNIFORM_TYPE)
    }

    fn set_time(&mut self, _t: f64) {}

    fn apply(&self, material: &Material) {
        material.set_uniform(&self.0, self.1.clone());
    }
}

impl<T: UniformValue + Tweenable> Uniform for (String, Anim<T>) {
    fn uniform_pair(&self) -> (String, UniformType) {
        (self.0.clone(), T::UNIFORM_TYPE)
    }

    fn set_time(&mut self, t: f64) {
        self.1.set_time(t);
    }

    fn apply(&self, material: &Material) {
        material.set_uniform(&self.0, self.1.now());
    }
}

pub struct Effect {
    time_range: Range<f64>,
    t: f64,
    material: Material,
    defaults: Vec<Box<dyn Uniform>>,
    uniforms: Vec<Box<dyn Uniform>>,
    pub global: bool,
}

impl Effect {
    pub fn get_preset(name: &str) -> Option<&'static str> {
        SHADERS.get(name).copied()
    }

    pub fn get_rpe_preset(name: &str) -> Option<&'static str> {
        RPE_SHADERS.get(name).copied()
    }

    pub fn new(time_range: Range<f64>, shader: &str, uniforms: Vec<Box<dyn Uniform>>, global: bool) -> Result<Self> {
        static DEF_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"uniform\s+(\w+)\s+(\w+);\s+//\s+%([^%]+)%").unwrap());
        let defaults = DEF_REGEX
            .captures_iter(shader)
            .map(|caps| -> Result<Box<dyn Uniform>> {
                let type_name = caps.get(1).unwrap().as_str();
                let name = caps.get(2).unwrap().as_str().to_owned();
                let value = caps.get(3).unwrap().as_str();
                Ok(match type_name {
                    "float" => Box::new((name, value.parse::<f32>()?)),
                    "vec2" => Box::new((name, {
                        let (x, y) = value.split_once(',').ok_or_else(|| anyhow!("Expected x,y"))?;
                        vec2(x.trim().parse()?, y.trim().parse()?)
                    })),
                    "vec4" => Box::new((name, {
                        let values: Vec<_> = value.split(',').map(|it| it.trim()).collect();
                        if values.len() != 4 {
                            bail!("Expected r,g,b,a");
                        }
                        Color::new(values[0].parse()?, values[1].parse()?, values[2].parse()?, values[3].parse()?)
                    })),
                    _ => bail!("Unknown type: {type_name}"),
                })
            })
            .collect::<Result<Vec<Box<dyn Uniform>>>>()?;
        let mut ocurred_uniforms = HashSet::new();
        let mut new_uniforms = Vec::new();
        let mut add_uniform = |(name, its_type): (String, UniformType)| {
            if ocurred_uniforms.insert(name.clone()) {
                new_uniforms.push((name, its_type));
            }
        };
        for def in &defaults {
            add_uniform(def.uniform_pair());
        }
        add_uniform(("time".to_owned(), UniformType::Float1));
        add_uniform(("screenSize".to_owned(), UniformType::Float2));
        add_uniform(("UVScale".to_owned(), UniformType::Float2));
        for u in &uniforms {
            add_uniform(u.uniform_pair());
        }
        Ok(Self {
            time_range,
            t: f64::NEG_INFINITY,
            defaults,
            material: load_material(
                ShaderSource::Glsl { vertex: VERTEX_SHADER, fragment: shader },
                MaterialParams {
                    uniforms: new_uniforms.into_iter().map(|(name, ty)| UniformDesc { name, uniform_type: ty, array_count: 1 }).collect(),
                    textures: vec!["screenTexture".to_owned()],
                    ..Default::default()
                },
            )?,
            uniforms,
            global,
        })
    }

    pub fn update(&mut self, res: &Resource) {
        let t = res.time;
        self.t = t;
        if self.time_range.contains(&t) {
            for uniform in &mut self.uniforms {
                uniform.set_time(t);
            }
        }
    }

    pub fn render(&self, res: &mut Resource) {
        if !self.time_range.contains(&self.t) {
            return;
        }
        let mut gl = unsafe { get_internal_gl() };
        gl.flush();

        for def in &self.defaults {
            def.apply(&self.material);
        }
        for uniform in &self.uniforms {
            uniform.apply(&self.material);
        }
        self.material.set_uniform("time", self.t as f32);
        let target = res.chart_target.as_mut().unwrap();
        target.swap();
        let tex = target.old().texture;
        self.material.set_texture("screenTexture", tex.clone());
        let screen_dim = vec2(tex.width(), tex.height());
        self.material.set_uniform("screenSize", screen_dim);
        gl.quad_gl.render_pass(Some(target.output().render_pass.raw_miniquad_id()));

        let vp = get_viewport();
        self.material.set_uniform("UVScale", vec2(vp.2 as _, vp.3 as _) / screen_dim);

        gl_use_material(&self.material);
        let top = 1. / if self.global { screen_aspect() } else { res.aspect_ratio };
        draw_rectangle(-1., -top, 2., top * 2., WHITE);
        gl_use_default_material();
    }
}

impl Drop for Effect {
    fn drop(&mut self) {}
}

const VERTEX_SHADER: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;

varying vec2 uv;

uniform mat4 Model;
uniform mat4 Projection;
uniform vec2 UVScale;

void main() {
    gl_Position = Projection * Model * vec4(position, 1);
    uv = (texcoord - vec2(0.5)) * UVScale + vec2(0.5);
}"#;
