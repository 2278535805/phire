use crate::{ffi, handle, AVCodecContext, AVCodecRef, AVFrame, AVPacket, AVPixelFormat, AudioStreamFormat, Error, OwnedPtr, Result, SwsContext, VideoStreamFormat};
use sasa::Frame;
use std::{ffi::CString, ptr::null_mut};

/// Encodes RGBA frames to an H.264 video stream in an MP4 container.
///
/// Audio frames are supplied as already mixed stereo PCM and encoded into the
/// AAC stream without involving the runtime audio backend.
pub struct VideoWriter {
    format: OwnedPtr<ffi::AVFormatContext>,
    codec: AVCodecContext,
    stream: *mut ffi::AVStream,
    audio_codec: AVCodecContext,
    audio_stream: *mut ffi::AVStream,
    packet: AVPacket,
    input: AVFrame,
    frame: AVFrame,
    audio_frame: AVFrame,
    audio_frame_size: usize,
    audio_pending: Vec<Frame>,
    audio_pts: i64,
    audio_format: AudioStreamFormat,
    sws: SwsContext,
    input_format: VideoStreamFormat,
    time_base: ffi::AVRational,
    finished: bool,
}

impl VideoWriter {
    pub fn new(path: impl AsRef<str>, width: i32, height: i32, fps: i32, bitrate: i64) -> Result<Self> {
        if width <= 0 || height <= 0 || fps <= 0 || width % 2 != 0 || height % 2 != 0 {
            return Err(Error::InvalidVideoFormat);
        }

        let encoder = AVCodecRef::find_encoder_by_name("libx264")?;
        let mut format = null_mut();
        let path = CString::new(path.as_ref()).map_err(|_| Error::InvalidPath)?;
        let mp4 = c"mp4";
        unsafe {
            handle(ffi::avformat_alloc_output_context2(&mut format, null_mut(), mp4.as_ptr(), path.as_ptr()))?;
        }
        let format = OwnedPtr::new(format).ok_or(Error::AllocationFailed)?;

        let mut codec = AVCodecContext::new_unconfigured(encoder)?;
        let audio_encoder = AVCodecRef::find_encoder(ffi::AV_CODEC_ID_AAC)?;
        let mut audio_codec = AVCodecContext::new_unconfigured(audio_encoder)?;
        let time_base = ffi::AVRational { num: 1, den: fps };
        let audio_time_base = ffi::AVRational { num: 1, den: 48_000 };
        unsafe {
            let ctx = codec.raw_mut();
            (*ctx).codec_type = 0;
            (*ctx).codec_id = ffi::AV_CODEC_ID_H264;
            (*ctx).width = width;
            (*ctx).height = height;
            (*ctx).pix_fmt = AVPixelFormat::YUV420P.0;
            (*ctx).time_base = time_base;
            (*ctx).framerate = ffi::AVRational { num: fps, den: 1 };
            (*ctx).bit_rate = bitrate.max(1);
            (*ctx).gop_size = fps.saturating_mul(2);
            (*ctx).max_b_frames = 0;
            (*ctx).flags |= ffi::AV_CODEC_FLAG_GLOBAL_HEADER;
            handle(ffi::avcodec_open2(ctx, encoder.raw(), null_mut()))?;

            let ctx = audio_codec.raw_mut();
            (*ctx).codec_type = 1;
            (*ctx).codec_id = ffi::AV_CODEC_ID_AAC;
            (*ctx).sample_fmt = ffi::AV_SAMPLE_FMT_FLTP;
            (*ctx).sample_rate = 48_000;
            (*ctx).ch_layout = ffi::AV_CHANNEL_LAYOUT_STEREO;
            (*ctx).time_base = audio_time_base;
            (*ctx).bit_rate = 320_000;
            (*ctx).flags |= ffi::AV_CODEC_FLAG_GLOBAL_HEADER;
            handle(ffi::avcodec_open2(ctx, audio_encoder.raw(), null_mut()))?;
        }

        let stream = unsafe { ffi::avformat_new_stream(format.0, encoder.raw()) };
        if stream.is_null() {
            return Err(Error::AllocationFailed);
        }
        let audio_stream = unsafe { ffi::avformat_new_stream(format.0, audio_encoder.raw()) };
        if audio_stream.is_null() {
            return Err(Error::AllocationFailed);
        }
        unsafe {
            (*stream).time_base = time_base;
            handle(ffi::avcodec_parameters_from_context((*stream).codecpar, codec.raw()))?;
            (*audio_stream).time_base = audio_time_base;
            handle(ffi::avcodec_parameters_from_context((*audio_stream).codecpar, audio_codec.raw()))?;
            handle(ffi::avio_open(&mut (*format.0).pb, path.as_ptr(), ffi::AVIO_FLAG_WRITE))?;
            handle(ffi::avformat_write_header(format.0, null_mut()))?;
        }

        let input_format = VideoStreamFormat {
            width,
            height,
            pix_fmt: AVPixelFormat::RGBA,
        };
        let output_format = VideoStreamFormat {
            width,
            height,
            pix_fmt: AVPixelFormat::YUV420P,
        };
        let mut frame = AVFrame::new()?;
        frame.set_video_format(&output_format);
        frame.get_buffer()?;
        let audio_frame_size = audio_codec.frame_size().max(1024) as usize;
        let audio_format = AudioStreamFormat {
            channel_layout: ffi::AV_CHANNEL_LAYOUT_STEREO,
            sample_fmt: ffi::AV_SAMPLE_FMT_FLTP,
            sample_rate: 48_000,
        };
        let mut audio_frame = AVFrame::new()?;
        audio_frame.set_audio_format(&audio_format);
        audio_frame.set_number_of_samples(audio_frame_size as i32);
        audio_frame.get_buffer()?;

        Ok(Self {
            format,
            codec,
            stream,
            audio_codec,
            audio_stream,
            packet: AVPacket::new()?,
            input: AVFrame::new()?,
            frame,
            audio_frame,
            audio_frame_size,
            audio_pending: Vec::new(),
            audio_pts: 0,
            audio_format,
            sws: SwsContext::new(input_format.clone(), output_format)?,
            input_format,
            time_base,
            finished: false,
        })
    }

    pub fn write_audio(&mut self, frames: &[Frame]) -> Result<()> {
        self.audio_pending.extend_from_slice(frames);
        while self.audio_pending.len() >= self.audio_frame_size {
            self.encode_audio_frame(self.audio_frame_size, false)?;
        }
        Ok(())
    }

    fn encode_audio_frame(&mut self, count: usize, pad: bool) -> Result<()> {
        self.audio_frame.unref();
        self.audio_frame.set_audio_format(&self.audio_format);
        self.audio_frame.set_number_of_samples(self.audio_frame_size as i32);
        self.audio_frame.get_buffer()?;
        unsafe {
            let data = self.audio_frame.0.as_mut().data;
            let left = data[0] as *mut f32;
            let right = data[1] as *mut f32;
            for i in 0..self.audio_frame_size {
                let sample = if i < count { self.audio_pending[i] } else if pad { Frame::default() } else { unreachable!() };
                left.add(i).write(sample.0);
                right.add(i).write(sample.1);
            }
        }
        self.audio_frame.set_pts(self.audio_pts);
        self.audio_pts += self.audio_frame_size as i64;
        self.audio_pending.drain(..count);
        self.audio_codec.send_frame(Some(&self.audio_frame))?;
        self.drain_audio_packets()
    }

    fn drain_audio_packets(&mut self) -> Result<()> {
        while self.audio_codec.receive_packet(&mut self.packet)? {
            unsafe {
                self.packet.rescale_ts(
                    ffi::AVRational { num: 1, den: 48_000 },
                    (*self.audio_stream).time_base,
                );
                self.packet.set_stream_index((*self.audio_stream).index);
                handle(ffi::av_interleaved_write_frame(self.format.0, self.packet.raw_mut()))?;
            }
            self.packet.unref();
        }
        Ok(())
    }

    pub fn write_rgba(&mut self, data: &[u8], stride: i32, pts: i64) -> Result<()> {
        if stride < self.input_format.width * 4 || data.len() < stride as usize * self.input_format.height as usize {
            return Err(Error::InvalidVideoFrame);
        }
        self.input.unref();
        self.input.set_external_video_buffer(&self.input_format, data.as_ptr() as *mut _, stride);
        self.frame.make_writable()?;
        self.frame.set_video_format(&VideoStreamFormat {
            width: self.input_format.width,
            height: self.input_format.height,
            pix_fmt: AVPixelFormat::YUV420P,
        });
        self.frame.set_pts(pts);
        self.sws.scale(&self.input, &mut self.frame)?;
        self.codec.send_frame(Some(&self.frame))?;
        self.drain_packets()
    }

    fn drain_packets(&mut self) -> Result<()> {
        while self.codec.receive_packet(&mut self.packet)? {
            unsafe {
                self.packet.rescale_ts(self.time_base, (*self.stream).time_base);
                self.packet.set_stream_index((*self.stream).index);
                handle(ffi::av_interleaved_write_frame(self.format.0, self.packet.raw_mut()))?;
            }
            self.packet.unref();
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<()> {
        self.finish_inner()
    }

    fn finish_inner(&mut self) -> Result<()> {
        if self.finished {
            return Ok(());
        }
        if !self.audio_pending.is_empty() {
            let count = self.audio_pending.len();
            self.encode_audio_frame(count, true)?;
        }
        self.audio_codec.send_frame(None)?;
        self.drain_audio_packets()?;
        self.codec.send_frame(None)?;
        self.drain_packets()?;
        unsafe {
            handle(ffi::av_write_trailer(self.format.0))?;
            if !(*self.format.0).pb.is_null() {
                handle(ffi::avio_closep(&mut (*self.format.0).pb))?;
            }
        }
        self.finished = true;
        Ok(())
    }
}

impl Drop for VideoWriter {
    fn drop(&mut self) {
        let _ = self.finish_inner();
        unsafe { ffi::avformat_free_context(self.format.0) }
    }
}
