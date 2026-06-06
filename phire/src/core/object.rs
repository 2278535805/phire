use crate::core::{AnimVector3, Vector3};

use super::{AnimFloat, AnimVector2, Matrix3, Resource, Vector2};
use macroquad::prelude::*;
use nalgebra::{Matrix4, Rotation2};

#[derive(Default)]
pub struct Object {
    pub alpha: AnimFloat,
    pub scale: AnimVector2,
    pub translation: AnimVector2,
    pub translation_z: AnimFloat,
    pub rotation_3d: AnimVector3,
    pub scale_z: AnimFloat,
}

impl Object {
    pub fn is_default(&self) -> bool {
        self.alpha.is_default()
            && self.scale.0.is_default()
            && self.scale.1.is_default()
            && self.translation.0.is_default()
            && self.translation.1.is_default()
            && self.translation_z.is_default()
            && self.rotation_3d.0.is_default() && self.rotation_3d.1.is_default() && self.rotation_3d.2.is_default()
            && self.scale_z.is_default()
    }

    pub fn set_time(&mut self, time: f64) {
        self.alpha.set_time(time);
        self.scale.0.set_time(time);
        self.scale.1.set_time(time);
        self.translation.0.set_time(time);
        self.translation.1.set_time(time);
        self.translation_z.set_time(time);
        self.rotation_3d.0.set_time(time);
        self.rotation_3d.1.set_time(time);
        self.rotation_3d.2.set_time(time);
        self.scale_z.set_time(time);
    }

    pub fn dead(&self) -> bool {
        self.alpha.dead()
            && self.scale.0.dead()
            && self.scale.1.dead()
            && self.translation.0.dead()
            && self.translation.1.dead()
            && self.translation_z.dead()
            && self.rotation_3d.0.dead() && self.rotation_3d.1.dead() && self.rotation_3d.2.dead()
            && self.scale_z.dead()
    }

    pub fn now(&self, res: &Resource) -> Matrix3 {
        self.now_rotation().append_translation(&self.now_translation(res))
    }

    pub fn now_3d(&self, res: &Resource) -> Matrix4<f32> {
        self.now_rotation_3d().append_translation(&self.now_translation_3d(res))
    }

    #[inline]
    pub fn now_rotation(&self) -> Matrix3 {
        Rotation2::new(self.rotation_3d.2.now().to_radians()).to_homogeneous()
    }

    #[inline]
    pub fn now_translation(&self, res: &Resource) -> Vector2 {
        let mut tr = self.translation.now();
        tr.y /= res.aspect_ratio;
        tr
    }

    pub fn now_translation_3d(&self, res: &Resource) -> Vector3 {
        let mut tr = self.translation.now();
        tr.y /= res.aspect_ratio;
        Vector3::new(tr.x, tr.y, self.translation_z.now())
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
        let sz = self.scale_z.now_opt().unwrap_or(1.0);
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
        let (rx, ry, rz) = (self.rotation_3d.0.now().to_radians(), self.rotation_3d.1.now().to_radians(), self.rotation_3d.2.now().to_radians());
        nalgebra::Rotation3::from_euler_angles(rx, ry, rz).to_homogeneous()
    }

    pub fn now_transform_3d(&self, res: &Resource) -> Matrix4<f32> {
        let tr = self.translation.now();
        let tr_z = self.translation_z.now();

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
