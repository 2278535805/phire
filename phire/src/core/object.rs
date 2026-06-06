use crate::core::{AnimVector3, Matrix4, Vector3};

use super::{AnimFloat, Matrix3, Resource, Vector2};
use macroquad::prelude::*;
use nalgebra::{Rotation2, Rotation3};

#[derive(Default)]
pub struct Object {
    pub alpha: AnimFloat,
    pub scale: AnimVector3,
    pub translation: AnimVector3,
    pub rotation: AnimVector3,
}

impl Object {
    pub fn is_default(&self) -> bool {
        self.alpha.is_default()
            && self.scale.0.is_default()
            && self.scale.1.is_default()
            && self.scale.2.is_default()
            && self.translation.0.is_default()
            && self.translation.1.is_default()
            && self.translation.2.is_default()
            && self.rotation.0.is_default() && self.rotation.1.is_default() && self.rotation.2.is_default()
    }

    pub fn set_time(&mut self, time: f64) {
        self.alpha.set_time(time);
        self.scale.0.set_time(time);
        self.scale.1.set_time(time);
        self.scale.2.set_time(time);
        self.translation.0.set_time(time);
        self.translation.1.set_time(time);
        self.translation.2.set_time(time);
        self.rotation.0.set_time(time);
        self.rotation.1.set_time(time);
        self.rotation.2.set_time(time);
    }

    pub fn dead(&self) -> bool {
        self.alpha.dead()
            && self.scale.0.dead()
            && self.scale.1.dead()
            && self.scale.2.dead()
            && self.translation.0.dead()
            && self.translation.1.dead()
            && self.translation.2.dead()
            && self.rotation.0.dead() && self.rotation.1.dead() && self.rotation.2.dead()
    }

    pub fn now(&self, res: &Resource) -> Matrix3 {
        self.now_rotation().to_homogeneous().append_translation(&self.now_translation(res))
    }

    pub fn now_3d(&self, res: &Resource) -> Matrix4 {
        self.now_rotation_3d().to_homogeneous().append_translation(&self.now_translation_3d(res))
    }

    #[inline]
    pub fn now_rotation(&self) -> Rotation2<f32> {
        Rotation2::new(self.rotation.2.now().to_radians())
    }

    pub fn now_rotation_3d(&self) -> Rotation3<f32> {
        let (rx, ry, rz) = (self.rotation.0.now().to_radians(), self.rotation.1.now().to_radians(), self.rotation.2.now().to_radians());
        Rotation3::from_euler_angles(rx, ry, rz)
    }

    #[inline]
    pub fn now_translation(&self, res: &Resource) -> Vector2 {
        let mut tr = self.translation.now();
        tr.y /= res.aspect_ratio;
        Vector2::new(tr.x, tr.y)
    }

    pub fn now_translation_3d(&self, res: &Resource) -> Vector3 {
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
        let scale = self.scale.now_with_def(1.0, 1.0, 1.0);
        Matrix3::identity().append_nonuniform_scaling(&Vector2::new(scale.x, scale.y))
    }

    #[inline]
    pub fn now_scale_3d(&self) -> Matrix4 {
        let scale = self.scale.now_with_def(1.0, 1.0, 1.0);
        Matrix4::identity().append_nonuniform_scaling(&scale)
    }

    pub fn now_scale_wrt_point(&self, scale_point: Vector2) -> Matrix3 {
        let scale = self.scale.now_with_def(1.0, 1.0, 1.0);
        let scale = Vector2::new(scale.x, scale.y);
        Matrix3::new_translation(&-scale_point).append_nonuniform_scaling(&scale).append_translation(&scale_point)
    }

    pub fn new_rotation_wrt_point(rot: Rotation2<f32>, pt: Vector2) -> Matrix3 {
        let translation_back = Matrix3::new_translation(&pt);
        let translation_to = Matrix3::new_translation(&-pt);
        translation_back * rot.to_homogeneous() * translation_to
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
