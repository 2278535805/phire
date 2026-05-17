use std::{f32, sync::Mutex, time::Duration};
use nalgebra::{Unit, UnitQuaternion, Vector3};
use lazy_static::lazy_static;

const GRAVITY_FLAT_THRESHOLD_LOW: f32 = 0.75;
const GRAVITY_FLAT_THRESHOLD_HIGH: f32 = 0.95;

#[derive(Debug, Clone, Copy)]
pub struct GyroData {
    pub angular_velocity: Vector3<f32>, // 角速度 (rad/s)
    pub timestamp: Duration,
}

pub struct Gyro {
    gravity: UnitQuaternion<f32>,
    gyroscope: UnitQuaternion<f32>,
    pub gyro_data: Option<GyroData>,
    flatness: f32, // 0.0 = 抬起 信任重力, 1.0 = 平放 信任陀螺仪
}

fn smooth_step(x: f32, edge0: f32, edge1: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let diff = (b - a + f32::consts::PI).rem_euclid(f32::consts::TAU) - f32::consts::PI;
    a + diff * t
}

lazy_static! {
    pub static ref GYRO: Mutex<Gyro> = Mutex::new(Gyro::new());
}

impl Gyro {
    pub fn new() -> Self {
        Self {
            gravity: UnitQuaternion::identity(),
            gyroscope: UnitQuaternion::identity(),
            gyro_data: None,
            flatness: 0.0,
        }
    }

    pub(crate) fn reset_gyroscope(&mut self) {
        let pitch = {
            // X轴在世界坐标系中的方向
            let world_x = self.gravity.transform_vector(&Vector3::new(1.0, 0.0, 0.0));
            // 绕Y轴的yaw
            world_x.z.atan2(world_x.x)
        };

        let yaw = if self.flatness <= 0.0 {
            let world_y = self.gravity.transform_vector(&Vector3::new(0.0, 1.0, 0.0));
            world_y.y.atan2(world_y.x)
        } else {
            0.0
        };

        self.gyroscope = UnitQuaternion::from_euler_angles(0.0, pitch, yaw);
    }

    pub fn update_gyroscope(&mut self, gyro_data: GyroData) {
        if let Some(last_gyro_data) = self.gyro_data {
            let dt = gyro_data.timestamp
                .saturating_sub(last_gyro_data.timestamp)
                .as_secs_f32();

            let omega = (last_gyro_data.angular_velocity + gyro_data.angular_velocity) / 2.0;
            let angle = omega.norm() * dt;

            if angle > 0.0 {
                let axis_unit: Unit<Vector3<f32>> = Unit::new_normalize(omega);
                let dq = UnitQuaternion::from_axis_angle(&axis_unit, angle); // 增量
                self.gyroscope *= dq;
            }
        }
        self.gyro_data = Some(gyro_data);
    }

    pub fn update_gravity(&mut self, gravity_data: Vector3<f32>) {
        let norm = gravity_data.norm();
        if norm == 0.0 {
            return;
        }

        let g_dev = gravity_data / norm; // 归一化 指向重力方向

        self.flatness = smooth_step(g_dev.z.abs(), GRAVITY_FLAT_THRESHOLD_LOW, GRAVITY_FLAT_THRESHOLD_HIGH);
        if self.flatness <= 0.0 {
            self.reset_gyroscope();
        }

        let world_gravity = Vector3::new(0.0, -1.0, 0.0); // 世界坐标系下的重力方向

        // g_dev 到 world_gravity 的旋转
        let q = UnitQuaternion::rotation_between(&g_dev, &world_gravity)
            .unwrap_or_else(UnitQuaternion::identity);

        self.gravity = q;
    }

    fn get_gyroscope_angle(&self) -> f32 {
        let (_, _, yaw) = self.gyroscope.to_rotation_matrix().euler_angles();
        yaw
    }

    fn get_gravity_angle(&self) -> f32 {
        let world = self.gravity.transform_vector(&Vector3::new(0.0, 1.0, 0.0));

        let proj = Vector3::new(world.x, world.y, world.z);
        let tan = world.y.atan2(proj.x);
        tan
    }

    pub fn get_angle(&self) -> f32 {
        let gravity_angle = self.get_gravity_angle();
        let gyro_angle = self.get_gyroscope_angle();
        lerp_angle(gravity_angle, gyro_angle, self.flatness)
    }

    pub fn get_current_acceleration(&self) -> f32 {
        self.gyro_data.map(|d| d.angular_velocity.norm()).unwrap_or(0.0)
    }
}
