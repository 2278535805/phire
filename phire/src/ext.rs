use crate::{
    config::Config,
    core::{Matrix, Point, Vector},
    ui::Ui,
};
use anyhow::{anyhow, Result};
use image::DynamicImage;
use lyon::{
    math::Box2D,
    path::{builder::BorderRadii, Path, Winding},
};
use macroquad::prelude::*;
use macroquad::miniquad::{gl::GLenum, BlendFactor, BlendState, BlendValue, CompareFunc, Equation, PrimitiveType, StencilFaceState, StencilOp, StencilState};
use once_cell::sync::Lazy;
use ordered_float::{Float, NotNan};
use regex::Regex;
use sasa::AudioManager;
use serde::Deserialize;
use std::{
    collections::VecDeque, future::Future, ops::Deref, pin::Pin, sync::{Arc, Mutex}, task::{Poll, RawWaker, RawWakerVTable, Waker}
};
use tracing::{debug, info_span};
use lazy_static::lazy_static;

pub type LocalTask<R> = Option<Pin<Box<dyn Future<Output = R>>>>;

pub trait JoinToString {
    fn join(self, sep: &str) -> String;
}

impl<V: AsRef<str>, T: Iterator<Item = V>> JoinToString for T {
    fn join(mut self, sep: &str) -> String {
        let mut result = String::new();
        if let Some(first) = self.next() {
            result += first.as_ref();
            for element in self {
                result += sep;
                result += element.as_ref();
            }
        }
        result
    }
}

pub trait NotNanExt: Sized {
    fn not_nan(self) -> NotNan<Self>;
}

impl<T: Sized + Float> NotNanExt for T {
    fn not_nan(self) -> NotNan<Self> {
        NotNan::new(self).unwrap()
    }
}

pub trait RectExt: Sized {
    fn feather(&self, radius: f32) -> Self;
    fn nonuniform_feather(&self, x: f32, y: f32) -> Self;
    fn to_euclid(&self) -> Box2D;
    fn rounded(&self, radius: f32) -> Path;
}

impl RectExt for Rect {
    fn feather(&self, radius: f32) -> Self {
        Self::new(self.x - radius, self.y - radius, self.w + radius * 2., self.h + radius * 2.)
    }

    fn nonuniform_feather(&self, x: f32, y: f32) -> Self {
        Self::new(self.x - x, self.y - y, self.w + x * 2., self.h + y * 2.)
    }

    fn to_euclid(&self) -> Box2D {
        Box2D::new(lyon::math::point(self.x, self.y), lyon::math::point(self.right(), self.bottom()))
    }

    fn rounded(&self, radius: f32) -> Path {
        let mut path = Path::builder();
        path.add_rounded_rectangle(&self.to_euclid(), &BorderRadii::new(radius), Winding::Positive);
        path.build()
    }
}

struct SafeTextureInner(Texture2D);

pub struct SafeTexture(Arc<SafeTextureInner>);
impl SafeTexture {
    pub fn into_inner(self) -> Texture2D {
        let arc = self.0;
        let res = arc.0.clone();
        std::mem::forget(arc);
        res
    }

    pub fn with_mipmap(self) -> Self {
        let macroquad::miniquad::RawId::OpenGl(id) = unsafe { get_internal_gl().quad_context.texture_raw_id(self.0 .0.raw_miniquad_id()) };
        unsafe {
            use macroquad::miniquad::gl::*;
            glBindTexture(GL_TEXTURE_2D, id);
            glGenerateMipmap(GL_TEXTURE_2D);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR_MIPMAP_LINEAR as _);
        }
        self
    }

    pub fn with_filter(self, filter: GLenum) -> Self{
        let macroquad::miniquad::RawId::OpenGl(id) = unsafe { get_internal_gl().quad_context.texture_raw_id(self.0 .0.raw_miniquad_id()) };
        unsafe {
            use macroquad::miniquad::gl::*;
            glBindTexture(GL_TEXTURE_2D, id);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, filter as _);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, filter as _);
        }
        self
    }

}

impl Clone for SafeTexture {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl Deref for SafeTexture {
    type Target = Texture2D;

    fn deref(&self) -> &Self::Target {
        &self.0.as_ref().0
    }
}

impl PartialEq for SafeTexture {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for SafeTexture {}

impl From<Texture2D> for SafeTexture {
    fn from(tex: Texture2D) -> Self {
        Self(Arc::new(SafeTextureInner(tex)))
    }
}

impl From<DynamicImage> for SafeTexture {
    fn from(image: DynamicImage) -> Self {
        Texture2D::from_rgba8(image.width() as _, image.height() as _, &image.into_rgba8()).into()
    }
}

pub static BLACK_TEXTURE: Lazy<SafeTexture> = Lazy::new(|| Texture2D::from_rgba8(1, 1, &[0, 0, 0, 255]).into());

pub fn nalgebra_to_glm(mat: &Matrix) -> Mat4 {
    /*
        [11] [12]  0  [13]
        [21] [22]  0  [23]
          0    0   1    0
        [31] [32]  0  [33]
    */
    Mat4::from_cols_array(&[
        mat.m11, mat.m21, 0., mat.m31, mat.m12, mat.m22, 0., mat.m32, 0., 0., 1., 0., mat.m13, mat.m23, 0., mat.m33,
    ])
}

pub fn get_viewport() -> (i32, i32, i32, i32) {
    let gl = unsafe { get_internal_gl() };
    let vp = gl.quad_gl.get_viewport();
    if vp.2 == 0 || vp.3 == 0 {
        (0, 0, screen_width() as _, screen_height() as _)
    } else {
        vp
    }
}

#[inline]
pub fn draw_text_aligned(ui: &mut Ui, text: &str, x: f32, y: f32, anchor: (f32, f32), scale: f32, color: Color) -> Rect {
    ui.text(text).pos(x, y).anchor(anchor.0, anchor.1).size(scale).color(color).multiline().draw()
}

pub fn draw_text_aligned_opt_width(ui: &mut Ui, text: &str, x: f32, y: f32, anchor: (f32, f32), mut scale: f32, color: Color, max_width: f32) -> Rect {
    let mut text = ui.text(text).size(scale).pos(x, y).anchor(anchor.0, anchor.1).color(color).multiline();
    let text_width = text.measure().w;
    if text_width > max_width {
        scale *= max_width / text_width
    }
    text.size(scale).draw()
}

pub fn draw_text_aligned_opt(ui: &mut Ui, text: &str, x: f32, y: f32, anchor: (f32, f32), mut scale: f32, color: Color, max_width: f32, max_height: f32) -> Rect {
    let r = ui.text(text).size(scale).multiline().measure();
    let (text_width, text_height) = (r.w, r.h);
    if text_width > max_width {
        scale *= max_width / text_width
    }
    if text_height > max_height {
        scale *= max_height / text_height
    }
    ui.text(text).pos(x, y).anchor(anchor.0, anchor.1).size(scale).color(color).multiline().draw()
}

#[derive(Debug, Default, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScaleType {
    #[default]
    CropCenter,
    Inside,
    Fit,
}

pub fn source_of_image(tex: &Texture2D, rect: Rect, scale_type: ScaleType) -> Option<Rect> {
    match scale_type {
        ScaleType::CropCenter => {
            let exp = rect.w / rect.h;
            let act = tex.width() / tex.height();
            Some(if exp > act {
                let h = act / exp;
                Rect::new(0., 0.5 - h / 2., 1., h)
            } else {
                let w = exp / act;
                Rect::new(0.5 - w / 2., 0., w, 1.)
            })
        }
        ScaleType::Inside => {
            let exp = rect.w / rect.h;
            let act = tex.width() / tex.height();
            Some(if exp > act {
                let w = exp / act;
                Rect::new(0.5 - w / 2., 0., w, 1.)
            } else {
                let h = act / exp;
                Rect::new(0., 0.5 - h / 2., 1., h)
            })
        }
        ScaleType::Fit => None,
    }
}

pub fn draw_image(tex: &Texture2D, rect: Rect, scale_type: ScaleType) {
    let source = source_of_image(tex, rect, scale_type);
    let (w, h) = (tex.width(), tex.height());
    draw_texture_ex(
        tex,
        rect.x,
        rect.y,
        WHITE,
        DrawTextureParams {
            source: source.map(|it| Rect::new(it.x * w, it.y * h, it.w * w, it.h * h)),
            dest_size: Some(rect.size()),
            ..Default::default()
        },
    );
}

pub const PARALLELOGRAM_SLOPE: f32 = 15.0 * std::f32::consts::PI / 180.0;

pub fn draw_parallelogram(rect: Rect, texture: Option<(&Texture2D, Rect)>, color: Color, shadow: bool) {
    draw_parallelogram_ex(rect, texture, color, color, shadow);
}

pub fn draw_parallelogram_ex(rect: Rect, texture: Option<(&Texture2D, Rect)>, top: Color, bottom: Color, shadow: bool) {
    let l = rect.h * PARALLELOGRAM_SLOPE;
    let gl = unsafe { get_internal_gl() }.quad_gl;
    let p = [
        Point::new(rect.x + l, rect.y),
        Point::new(rect.right(), rect.y),
        Point::new(rect.x, rect.bottom()),
        Point::new(rect.right() - l, rect.bottom()),
    ];
    let v = if let Some((tex, tex_rect)) = texture {
        let lt = tex_rect.h * tex.height() * PARALLELOGRAM_SLOPE / tex.width();
        gl.texture(Some(tex));
        [
            Vertex::new(p[0].x, p[0].y, 0., tex_rect.x + lt, tex_rect.y, top),
            Vertex::new(p[1].x, p[1].y, 0., tex_rect.right(), tex_rect.y, top),
            Vertex::new(p[2].x, p[2].y, 0., tex_rect.x, tex_rect.bottom(), bottom),
            Vertex::new(p[3].x, p[3].y, 0., tex_rect.right() - lt, tex_rect.bottom(), bottom),
        ]
    } else {
        gl.texture(None);
        [
            Vertex::new(p[0].x, p[0].y, 0., 0., 0., top),
            Vertex::new(p[1].x, p[1].y, 0., 0., 0., top),
            Vertex::new(p[2].x, p[2].y, 0., 0., 0., bottom),
            Vertex::new(p[3].x, p[3].y, 0., 0., 0., bottom),
        ]
    };
    gl.draw_mode(DrawMode::Triangles);
    gl.geometry(&v, &[0, 2, 3, 0, 1, 3]);
    if shadow {
        drop_shadow(p, top.a.min(bottom.a));
        //drop_shadow(p, 1.0);
    }
}

pub fn draw_illustration(tex: &Texture2D, x: f32, y: f32, w: f32, h: f32, color: Color, shadow: bool) -> Rect {
    let scale = 0.076;
    let w = scale * 13. * w;
    let h = scale * 7. * h;
    let r = Rect::new(x - w / 2., y - h / 2., w, h);
    let tr = {
        let exp = w / h;
        let act = tex.width() / tex.height();
        if exp > act {
            let h = act / exp;
            Rect::new(0., 0.5 - h / 2., 1., h)
        } else {
            let w = exp / act;
            Rect::new(0.5 - w / 2., 0., w, 1.)
        }
    };
    crate::ext::draw_parallelogram(r, Some((tex, tr)), color, shadow);
    r
}


fn drop_shadow(p: [Point; 4], alpha: f32) {
    const RADIUS: f32 = 0.018;
    let len = (PARALLELOGRAM_SLOPE * PARALLELOGRAM_SLOPE + 1.).sqrt();
    let n1 = Vector::new(PARALLELOGRAM_SLOPE / len - 1., -1. / len) * RADIUS;
    let n2 = Vector::new(n1.x + RADIUS * 2., n1.y);
    let c1 = Color::new(0., 0., 0., alpha * 0.06);
    //let c2 = Color::default();
    let c2 = Color::new(0., 0., 0., 0.);
    let v = |p: Point, c: Color| Vertex::new(p.x, p.y, 0., 0., 0., c);
    let p = [
        v(p[0], c1),
        v(p[0] + n1, c2),
        v(p[1], c1),
        v(p[1] + n2, c2),
        v(p[2], c1),
        v(p[2] - n2, c2),
        v(p[3], c1),
        v(p[3] - n1, c2),
    ];
    let gl = unsafe { get_internal_gl() }.quad_gl;
    gl.texture(None);
    gl.draw_mode(DrawMode::Triangles);
    gl.geometry(&p, &[0, 1, 2, 1, 2, 3, 0, 1, 5, 0, 5, 4, 4, 5, 6, 5, 6, 7, 6, 7, 2, 7, 2, 3]);
}

pub fn rect_shadow(r: Rect, radius: f32, alpha: f32) {
    let t = r.feather(radius);
    let v = |x: f32, y: f32, c: Color| Vertex::new(x, y, 0., 0., 0., c);
    let a = Color::new(0., 0., 0., alpha);
    let b = Color::default();
    let p = [
        v(t.x, t.y, b),
        v(t.right(), t.y, b),
        v(r.x, r.y, a),
        v(r.right(), r.y, a),
        v(r.x, r.bottom(), a),
        v(r.right(), r.bottom(), a),
        v(t.x, t.bottom(), b),
        v(t.right(), t.bottom(), b),
    ];
    let gl = unsafe { get_internal_gl() }.quad_gl;
    gl.texture(None);
    gl.draw_mode(DrawMode::Triangles);
    gl.geometry(&p, &[0, 1, 2, 1, 2, 3, 0, 2, 4, 4, 0, 6, 4, 5, 6, 5, 6, 7, 1, 3, 5, 5, 1, 7]);
}

pub fn thread_as_future<R: Send + 'static>(f: impl FnOnce() -> R + Send + 'static) -> impl Future<Output = R> {
    struct DummyFuture<R>(Arc<Mutex<Option<R>>>);
    impl<R> Future for DummyFuture<R> {
        type Output = R;

        fn poll(self: std::pin::Pin<&mut Self>, _: &mut std::task::Context<'_>) -> Poll<Self::Output> {
            match self.0.lock().unwrap().take() {
                Some(res) => Poll::Ready(res),
                None => Poll::Pending,
            }
        }
    }
    let arc = Arc::new(Mutex::new(None));
    std::thread::spawn({
        let arc = Arc::clone(&arc);
        move || {
            let res = f();
            *arc.lock().unwrap() = Some(res);
        }
    });
    DummyFuture(arc)
}

pub fn poll_future<R>(future: Pin<&mut (impl Future<Output = R> + ?Sized)>) -> Option<R> {
    fn waker() -> Waker {
        unsafe fn clone(data: *const ()) -> RawWaker {
            RawWaker::new(data, &VTABLE)
        }
        unsafe fn wake(_data: *const ()) {
            // panic!()
        }
        unsafe fn wake_by_ref(data: *const ()) {
            wake(data)
        }
        unsafe fn drop(_data: *const ()) {}
        const VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);
        let raw_waker = RawWaker::new(std::ptr::null(), &VTABLE);
        unsafe { Waker::from_raw(raw_waker) }
    }
    let waker = waker();
    let mut futures_context = std::task::Context::from_waker(&waker);
    match future.poll(&mut futures_context) {
        Poll::Ready(val) => Some(val),
        Poll::Pending => None,
    }
}

pub fn screen_aspect() -> f32 {
    let vp = get_viewport();
    vp.2 as f32 / vp.3 as f32
}

pub fn create_audio_manger(config: &Config) -> Result<AudioManager> {
    #[cfg(target_os = "android")]
    {
        use sasa::backend::oboe::*;
        let sharing_mode = if config.audio_compatibility {
            SharingMode::Shared
        } else {
            SharingMode::Exclusive
        };
        let usage = if config.audio_compatibility {
            Usage::Media
        } else {
            Usage::Game
        };
        let mmap = if config.audio_compatibility {
            false
        } else {
            true
        };
        AudioManager::new(OboeBackend::new(OboeSettings {
            buffer_size: config.audio_buffer_size,
            performance_mode: PerformanceMode::LowLatency,
            sharing_mode,
            usage,
            mmap,
            ..Default::default()
        }))
    }
    #[cfg(target_os = "windows")]
    {
        use sasa::backend::wasapi::*;
        let share_mode = if config.audio_compatibility {
            ShareMode::Shared
        } else {
            ShareMode::Exclusive
        };
        AudioManager::new(WasapiBackend::new(WasapiSettings {
            buffer_size: config.audio_buffer_size,
            share_mode,
            raw_stream: true,
            ..Default::default()
        }))
    }
    #[cfg(not(any(target_os = "android", target_os = "windows")))]
    {
        use sasa::backend::cpal::*;
        Ok(AudioManager::new(CpalBackend::new(CpalSettings {
            buffer_size: config.audio_buffer_size,
        }))
        .expect("Failed to play sound"))
    }
}

pub fn make_pipeline(write_color: bool, pass_op: StencilOp, test_func: CompareFunc, test_ref: i32) -> GlPipeline {
    let InternalGlContext {
        quad_gl: gl,
        quad_context: context,
    } = unsafe { get_internal_gl() };
    gl.make_pipeline(
        context,
        ShaderSource::Glsl { vertex: shader::VERTEX, fragment: shader::FRAGMENT },
        PipelineParams {
            color_write: (write_color, write_color, write_color, write_color),
            color_blend: Some(BlendState::new(
                Equation::Add,
                BlendFactor::Value(BlendValue::SourceAlpha),
                BlendFactor::OneMinusValue(BlendValue::SourceAlpha),
            )),
            stencil_test: {
                let state = StencilFaceState {
                    fail_op: StencilOp::Keep,
                    depth_fail_op: StencilOp::Keep,
                    pass_op,
                    test_func,
                    test_ref,
                    test_mask: u32::MAX,
                    write_mask: u32::MAX,
                };
                Some(StencilState { front: state, back: state })
            },
            primitive_type: PrimitiveType::Triangles,
            ..Default::default()
        },
        Vec::new(),
        Vec::new(),
    )
    .unwrap()
}

#[inline]
pub fn semi_black(alpha: f32) -> Color {
    Color { a: alpha, ..BLACK }
}

#[inline]
pub fn semi_white(alpha: f32) -> Color {
    Color { a: alpha, ..WHITE }
}

pub fn open_url(url: &str) -> Result<()> {
    cfg_if::cfg_if! {
        if #[cfg(target_os = "android")] {
            unsafe {
                let env = macroquad::miniquad::native::attach_jni_env();
                let ctx = ndk_context::android_context().context();
                let class = (**env).GetObjectClass.unwrap()(env, ctx);
                let method =
                    (**env).GetMethodID.unwrap()(env, class, b"openUrl\0".as_ptr() as _, b"(Ljava/lang/String;)V\0".as_ptr() as _);
                let url = std::ffi::CString::new(url.to_owned()).unwrap();
                (**env).CallVoidMethod.unwrap()(
                    env,
                    ctx,
                    method,
                    (**env).NewStringUTF.unwrap()(env, url.as_ptr()),
                );
            }
        } else if #[cfg(target_os = "ios")] {
            unsafe {
                use crate::objc::*;

                let application: ObjcId = msg_send![class!(UIApplication), sharedApplication];
                let url: ObjcId = msg_send![class!(NSURL), URLWithString: str_to_ns(url)];
                let _: () = msg_send![application, openURL: url];
            }
        } else {
            open::that(url)?;
        }
    }

    Ok(())
}


pub fn unzip_into<R: std::io::Read + std::io::Seek>(reader: R, dir: &crate::dir::Dir, strip_root: bool) -> Result<()> {
    let mut zip = zip::ZipArchive::new(reader)?;
    let root = if strip_root {
        if let Some(root) = zip.file_names().min_by_key(|it| it.len()) {
            if root.ends_with('/') && zip.file_names().all(|it| it.starts_with(root)) {
                root.to_owned()
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    } else {
        String::new()
    };
    let _span = info_span!("unzip").entered();
    debug!("root is {root}");
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let path = entry.enclosed_name().ok_or_else(|| anyhow!("invalid zip"))?;
        let path = path.display().to_string();
        debug!("entry: {path}");
        if entry.is_dir() && entry.name() != root {
            if let Some(after) = path.strip_prefix(&root) {
                debug!("mkdir: {after}");
                dir.create_dir_all(after)?;
            }
        } else if entry.is_file() {
            if let Some(after) = path.strip_prefix(&root) {
                if let Some(p) = std::path::Path::new(after).parent() {
                    if !dir.exists(p)? {
                        debug!("mkdir {}", p.display());
                        dir.create_dir_all(p)?;
                    }
                }
                debug!("create {}", after);
                let mut file = dir.create(after)?;
                std::io::copy(&mut entry, &mut file)?;
            }
        }
    }
    Ok(())
}

pub fn parse_time(s: &str) -> Option<f64> {
    if s.is_empty() {
        return None;
    }
    let r = s.split(':').collect::<Vec<_>>();
    if r.len() > 3 {
        return None;
    }
    let mut iter = r.into_iter().rev();
    let mut res = iter.next().unwrap().parse::<f64>().ok()?;
    if res < 0. {
        return None;
    }
    if let Some(mins) = iter.next() {
        res += mins.parse::<u32>().ok()? as f64 * 60.;
    }
    if let Some(hrs) = iter.next() {
        res += hrs.parse::<u32>().ok()? as f64 * 3600.;
    }
    Some(res)
}

pub fn parse_alpha(alpha: f32, force_alpha: f32, min_alpha: f32, chart_debug: bool) -> f32 {
    if chart_debug {
        (min_alpha + (1. - min_alpha) * alpha) * force_alpha
    } else {
        alpha * force_alpha
    }
}

pub fn ease_in_out_cubic(t: f32) -> f32 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        let t = t * 2.0 - 2.0;
        0.5 * t * t * t + 1.0
    }
}

pub fn ease_in_out_quartic(t: f32) -> f32 {
    if t < 0.5 {
        8.0 * t * t * t * t
    } else {
        let t = t * 2.0 - 2.0;
        1.0 - 0.5 * t * t * t * t
    }
}

pub fn ease_in_out_quintic(t: f32) -> f32 {
    if t < 0.5 {
        16.0 * t * t * t * t * t
    } else {
        let t = t * 2.0 - 2.0;
        0.5 * t * t * t * t * t + 1.0
    }
}

lazy_static! {
    static ref RE_FILTER: Regex = Regex::new(r##"[^a-zA-Z0-9!"'#$%&'()*+,\-.\/:;<=>?@\\\[\]^_`{|}~ΜΟΒСՕⅭОмвＣＯＭＢМⅯВϹȮΌϺϺƁ]"##).unwrap();
    static ref RE_VALIDATE: Regex = Regex::new(r"^[CСⅭＣϹ][OՕΟ0ОＯȮΌ][MΜмＭМⅯϺϺ][BΒ8вＢВ][OՕΟ0ОＯȮΌ]$").unwrap();
}

pub fn validate_combo(value: &String) -> bool {
    if value == "AUTOPLAY" || value == "RECORD" {
        return false;
    }

    let filtered_value = RE_FILTER.replace_all(value, "").trim().to_string();
    RE_VALIDATE.is_match(&filtered_value)
}

pub fn get_frame_latency(frame_times: &VecDeque<f64>) -> f64 {
    (1.0 / frame_times.len() as f64).min(0.25)
}

pub fn get_audio_latency(audio: &AudioManager) -> f64 {
    audio.estimate_latency().max(0.)
}

pub fn push_frame_time(frame_times: &mut VecDeque<f64>, real_time: f64) {
    frame_times.push_back(real_time);
    while frame_times.front().is_some_and(|it| real_time - it > 1.0) {
        frame_times.pop_front();
    }
}

pub fn round_to_step(value: f32, step: f32) -> f32 {
    let aligned = (value / step).round() * step;
    let digits = (-step.log10()).ceil() as i32;
    let factor = 10_f32.powi(digits);
    (aligned * factor).round() / factor
}

pub fn blur_image(image: DynamicImage, blur: f32) -> Result<SafeTexture> {
    let (w, h) = (image.width(), image.height());
    let size = w as usize * h as usize;
    let mut blurred_rgb = image.to_rgb8();
    let mut vec = unsafe { Vec::from_raw_parts(std::mem::transmute(blurred_rgb.as_mut_ptr()), size, size) };
    fastblur::gaussian_blur(&mut vec, w as _, h as _, blur);
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
        Texture2D::from_image(&Image {
            width: w as _,
            height: h as _,
            bytes: blurred,
        })
    ).into())

}


mod shader {
    pub const VERTEX: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;

varying lowp vec2 uv;
varying lowp vec4 color;

uniform mat4 Model;
uniform mat4 Projection;

void main() {
    gl_Position = Projection * Model * vec4(position, 1);
    color = color0 / 255.0;
    uv = texcoord;
}"#;

    pub const FRAGMENT: &str = r#"#version 100
varying lowp vec4 color;
varying lowp vec2 uv;

uniform sampler2D Texture;

void main() {
    gl_FragColor = color * texture2D(Texture, uv);
}"#;
}
