use macroquad::{
    texture::{render_target, render_target_ex, RenderTarget, RenderTargetParams},
    window::get_internal_gl,
};
use macroquad::miniquad::{self as miniquad, gl::GLuint};

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

impl MSRenderTarget {
    pub fn new(dim: (u32, u32), samples: u32) -> Self {
        let input = render_target_ex(
            dim.0,
            dim.1,
            RenderTargetParams {
                sample_count: samples as i32,
                ..Default::default()
            },
        );
        let output = render_target(dim.0, dim.1);
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
            self.output[0] = Some(render_target(self.dim.0, self.dim.1));
            // TODO: copy content from old output to new output
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
