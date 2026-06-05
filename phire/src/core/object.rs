use crate::core::Vector3;

use super::{AnimFloat, AnimVector, Matrix3, Resource, Vector2};
use macroquad::prelude::*;
use nalgebra::{Matrix4, Rotation2};

#[derive(Default)]
pub struct Object {
    pub alpha: AnimFloat,
    pub scale: AnimVector,
    pub rotation: AnimFloat,
    pub translation: AnimVector,
    pub translation_z: Option<AnimFloat>,
    pub rotation_3d: Option<(AnimFloat, AnimFloat, AnimFloat)>,
    pub scale_z: Option<AnimFloat>,
}

impl Object {
    pub fn is_default(&self) -> bool {
        self.alpha.is_default()
            && self.scale.0.is_default()
            && self.scale.1.is_default()
            && self.rotation.is_default()
            && self.translation.0.is_default()
            && self.translation.1.is_default()
            && self.translation_z.as_ref().map_or(true, |z| z.is_default())
            && self.rotation_3d.as_ref().map_or(true, |(x, y, z)| x.is_default() && y.is_default() && z.is_default())
            && self.scale_z.as_ref().map_or(true, |z| z.is_default())
    }

    pub fn has_3d(&self) -> bool {
        self.translation_z.is_some() || self.rotation_3d.is_some() || self.scale_z.is_some()
    }

    pub fn set_time(&mut self, time: f64) {
        self.alpha.set_time(time);
        self.scale.0.set_time(time);
        self.scale.1.set_time(time);
        self.rotation.set_time(time);
        self.translation.0.set_time(time);
        self.translation.1.set_time(time);
        if let Some(ref mut z) = self.translation_z {
            z.set_time(time);
        }
        if let Some((ref mut x, ref mut y, ref mut z)) = self.rotation_3d {
            x.set_time(time);
            y.set_time(time);
            z.set_time(time);
        }
        if let Some(ref mut z) = self.scale_z {
            z.set_time(time);
        }
    }

    pub fn dead(&self) -> bool {
        self.alpha.dead()
            && self.scale.0.dead()
            && self.scale.1.dead()
            && self.rotation.dead()
            && self.translation.0.dead()
            && self.translation.1.dead()
            && self.translation_z.as_ref().map_or(true, |z| z.dead())
            && self.rotation_3d.as_ref().map_or(true, |(x, y, z)| x.dead() && y.dead() && z.dead())
            && self.scale_z.as_ref().map_or(true, |z| z.dead())
    }

    pub fn now(&self, res: &Resource) -> Matrix3 {
        self.now_rotation().append_translation(&self.now_translation(res))
    }

    #[inline]
    pub fn now_rotation(&self) -> Matrix3 {
        Rotation2::new(self.rotation.now().to_radians()).to_homogeneous()
    }

    #[inline]
    pub fn now_translation(&self, res: &Resource) -> Vector2 {
        let mut tr = self.translation.now();
        tr.y /= res.aspect_ratio;
        tr
    }

    #[inline]
    pub fn now_alpha(&self) -> f32 {
        self.alpha.now_opt().unwrap_or(1.0)
    }

    #[inline]
    pub fn now_scale(&self) -> Matrix3 {
        Matrix3::identity().append_nonuniform_scaling(&self.scale.now_with_def(1.0, 1.0))
    }

    #[inline]
    pub fn now_scale_3d(&self) -> Matrix4<f32> {
        let scale = self.scale.now_with_def(1.0, 1.0);
        let sz = self.scale_z.as_ref().map_or(1.0, |z| z.now_opt().unwrap_or(1.0));
        Matrix4::identity().append_nonuniform_scaling(&Vector3::new(scale.x, scale.y, sz))
    }

    pub fn now_scale_wrt_point(&self, scale_point: Vector2) -> Matrix3 {
        let scale = self.scale.now_with_def(1.0, 1.0);
        Matrix3::new_translation(&-scale_point).append_nonuniform_scaling(&scale).append_translation(&scale_point)
    }

    pub fn new_rotation_wrt_point(rot: Rotation2<f32>, pt: Vector2) -> Matrix3 {
        let translation_back = Matrix3::new_translation(&pt);
        let translation_to = Matrix3::new_translation(&-pt);
        translation_back * rot.to_homogeneous() * translation_to
    }

    pub fn now_rotation_3d(&self) -> Matrix4<f32> {
        let (rx, ry, rz) = if let Some((ref rx, ref ry, ref rz)) = self.rotation_3d {
            (rx.now().to_radians(), ry.now().to_radians(), rz.now().to_radians())
        } else {
            (0.0, 0.0, self.rotation.now().to_radians())
        };

        nalgebra::Rotation3::from_euler_angles(rx, ry, rz).to_homogeneous()
    }

    pub fn now_transform_3d(&self, res: &Resource) -> Matrix4<f32> {
        let tr = self.translation.now();
        let tr_z = self.translation_z.as_ref().map_or(0.0, |z| z.now());

        let ar = res.aspect_ratio;
        let translation = Matrix4::new_translation(&nalgebra::Vector3::new(tr.x, tr.y / ar, tr_z));

        translation * self.now_rotation_3d() * self.now_scale_3d()
    }
}

#[derive(Default, Clone)]
pub struct CtrlObject {
    pub alpha: AnimFloat,
    pub size: AnimFloat,
    pub pos: AnimFloat,
    pub y: AnimFloat,
}

impl CtrlObject {
    pub fn set_height(&mut self, height: f64) {
        self.alpha.set_time(height);
        self.size.set_time(height);
        self.pos.set_time(height);
        self.y.set_time(height);
    }
}
