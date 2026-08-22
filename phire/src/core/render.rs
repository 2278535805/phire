use macroquad::miniquad::{self as miniquad, gl::GLuint};
use macroquad::{
    texture::{RenderTarget, Texture2D},
    window::get_internal_gl,
};
use std::ptr::null_mut;

pub struct MSRenderTarget {
    dim: (u32, u32),
    fbo: GLuint,
    input: RenderTarget,
    output: [Option<RenderTarget>; 2],
}

pub fn copy_fbo(src: GLuint, dst: GLuint, dim: (u32, u32)) -> bool {
    unsafe {
        use miniquad::gl::*;
        glBindFramebuffer(GL_READ_FRAMEBUFFER, src);
        glBindFramebuffer(GL_DRAW_FRAMEBUFFER, dst);
        let (w, h) = (dim.0 as i32, dim.1 as i32);
        glBlitFramebuffer(0, 0, w, h, 0, 0, w, h, GL_COLOR_BUFFER_BIT, GL_NEAREST);
        glGetError() == GL_NO_ERROR
    }
}

fn get_fbo(target: &RenderTarget) -> GLuint {
    let gl = unsafe { get_internal_gl() };
    let rp = target.render_pass.raw_miniquad_id();
    unsafe {
        gl.quad_context.begin_pass(Some(rp), miniquad::PassAction::Nothing);
        let mut fbo: GLuint = 0;
        miniquad::gl::glGetIntegerv(miniquad::gl::GL_FRAMEBUFFER_BINDING, &mut fbo as *mut _ as *mut _);
        gl.quad_context.end_render_pass();
        fbo
    }
}

pub fn internal_id(target: RenderTarget) -> GLuint {
    get_fbo(&target)
}

pub fn read_render_target_rgba8(target: RenderTarget, dim: (u32, u32), output: &mut Vec<u8>) {
    let gl = unsafe { get_internal_gl() };
    let size = dim.0 as usize * dim.1 as usize * 4;
    output.resize(size, 0);
    unsafe {
        gl.quad_context
            .begin_pass(Some(target.render_pass.raw_miniquad_id()), miniquad::PassAction::Nothing);
        miniquad::gl::glReadPixels(0, 0, dim.0 as i32, dim.1 as i32, miniquad::gl::GL_RGBA, miniquad::gl::GL_UNSIGNED_BYTE, output.as_mut_ptr() as _);
        gl.quad_context.end_render_pass();
    }
}

pub struct AsyncRgbaReadback {
    pbos: [GLuint; 5],
    next: usize,
    pending: usize,
    width: i32,
    height: i32,
    size: usize,
}

impl AsyncRgbaReadback {
    pub fn new(width: u32, height: u32) -> Self {
        let mut pbos = [0; 5];
        let size = width as usize * height as usize * 4;
        unsafe {
            use miniquad::gl::*;
            glGenBuffers(pbos.len() as _, pbos.as_mut_ptr());
            for pbo in pbos {
                glBindBuffer(GL_PIXEL_PACK_BUFFER, pbo);
                glBufferData(GL_PIXEL_PACK_BUFFER, size as _, null_mut(), GL_STREAM_READ);
            }
            glBindBuffer(GL_PIXEL_PACK_BUFFER, 0);
        }
        Self { pbos, next: 0, pending: 0, width: width as _, height: height as _, size }
    }

    pub fn read(&mut self, target: RenderTarget) -> Option<Vec<u8>> {
        let result = if self.pending == self.pbos.len() - 1 {
            let oldest = (self.next + self.pbos.len() - self.pending) % self.pbos.len();
            self.map(oldest)
        } else {
            None
        };
        unsafe {
            use miniquad::gl::*;
            glBindFramebuffer(GL_READ_FRAMEBUFFER, get_fbo(&target));
            glBindBuffer(GL_PIXEL_PACK_BUFFER, self.pbos[self.next]);
            glReadPixels(0, 0, self.width, self.height, GL_RGBA, GL_UNSIGNED_BYTE, null_mut());
            glBindBuffer(GL_PIXEL_PACK_BUFFER, 0);
        }
        self.next = (self.next + 1) % self.pbos.len();
        self.pending = (self.pending + 1).min(self.pbos.len() - 1);
        result
    }

    fn map(&self, index: usize) -> Option<Vec<u8>> {
        unsafe {
            use miniquad::gl::*;
            glBindBuffer(GL_PIXEL_PACK_BUFFER, self.pbos[index]);
            let ptr = glMapBuffer(GL_PIXEL_PACK_BUFFER, 0x88B8) as *const u8;
            let result = (!ptr.is_null()).then(|| std::slice::from_raw_parts(ptr, self.size).to_vec());
            glUnmapBuffer(GL_PIXEL_PACK_BUFFER);
            glBindBuffer(GL_PIXEL_PACK_BUFFER, 0);
            result
        }
    }

    pub fn drain(&mut self) -> Vec<Vec<u8>> {
        let mut result = Vec::with_capacity(self.pending);
        while self.pending > 0 {
            let oldest = (self.next + self.pbos.len() - self.pending) % self.pbos.len();
            if let Some(data) = self.map(oldest) {
                result.push(data);
            }
            self.pending -= 1;
        }
        result
    }
}

impl Drop for AsyncRgbaReadback {
    fn drop(&mut self) {
        unsafe { miniquad::gl::glDeleteBuffers(self.pbos.len() as _, self.pbos.as_ptr()); }
    }
}

fn create_render_target_rgb8(width: u32, height: u32, sample_count: i32) -> RenderTarget {
    let gl = unsafe { get_internal_gl() };
    let ctx = gl.quad_context;

    let color_texture = ctx.new_render_texture(miniquad::TextureParams {
        width,
        height,
        format: miniquad::TextureFormat::RGB8,
        sample_count,
        ..Default::default()
    });

    let render_pass = if sample_count > 1 {
        let resolve_texture = ctx.new_render_texture(miniquad::TextureParams {
            width,
            height,
            format: miniquad::TextureFormat::RGB8,
            sample_count: 1,
            ..Default::default()
        });
        ctx.new_render_pass_mrt(&[color_texture], Some(&[resolve_texture]), None)
    } else {
        ctx.new_render_pass(color_texture, None)
    };

    // Get the texture that contains the final result (resolve texture for MSAA, color texture otherwise)
    let result_texture_id = if sample_count > 1 {
        ctx.render_pass_color_attachments(render_pass)[0]
    } else {
        color_texture
    };

    RenderTarget {
        texture: Texture2D::from_miniquad_texture(result_texture_id),
        render_pass: macroquad::texture::RenderPass {
            color_texture: Texture2D::from_miniquad_texture(result_texture_id),
            depth_texture: None,
            render_pass: std::sync::Arc::new(render_pass),
        },
    }
}

impl MSRenderTarget {
    pub fn new(dim: (u32, u32), samples: u32) -> Self {
        let input = create_render_target_rgb8(dim.0, dim.1, samples as i32);
        let output = create_render_target_rgb8(dim.0, dim.1, 1);
        let fbo = get_fbo(&input);
        Self {
            dim,
            fbo,
            input,
            output: [Some(output), None],
        }
    }

    pub fn blit(&self) {
        if let Some(target) = &self.output[0] {
            let dst_fbo = get_fbo(target);
            copy_fbo(self.fbo, dst_fbo, self.dim);
        }
    }

    pub fn swap(&mut self) {
        self.output.swap(0, 1);
        if self.output[0].is_none() {
            self.output[0] = Some(create_render_target_rgb8(self.dim.0, self.dim.1, 1));
        }
    }

    pub fn input(&self) -> RenderTarget {
        self.input.clone()
    }

    pub fn output(&self) -> RenderTarget {
        self.output[0].clone().unwrap()
    }

    pub fn old(&self) -> RenderTarget {
        self.output[1].clone().unwrap()
    }
}

impl Drop for MSRenderTarget {
    fn drop(&mut self) {
        // Render pass and texture cleanup is handled by macroquad's RenderPass Drop impl
    }
}
