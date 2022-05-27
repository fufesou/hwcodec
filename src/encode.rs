use crate::ffmpeg::{
    encode, ffmpeg_linesize_offset_length, free_encoder, new_encoder, AVPixelFormat, CodecInfo,
    DataFormat::{self, *},
    Vendor::*,
    AV_NUM_DATA_POINTERS,
};
use log::{error, trace};
use std::{
    ffi::{c_void, CString},
    fmt::Display,
    os::raw::c_int,
    slice,
};

#[derive(Debug, Clone, PartialEq)]
pub struct EncodeContext {
    pub name: String,
    pub fps: i32,
    pub width: i32,
    pub height: i32,
    pub pixfmt: AVPixelFormat,
    pub align: i32,
}

pub struct EncodeFrame {
    pub data: Vec<u8>,
    pub pts: i64,
}

impl Display for EncodeFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "encode len:{}, pts:{}", self.data.len(), self.pts)
    }
}

pub struct Encoder {
    codec: Box<c_void>,
    frames: *mut Vec<EncodeFrame>,
    pub ctx: EncodeContext,
    pub linesize: Vec<i32>,
    pub offset: Vec<i32>,
    pub length: i32,
}

impl Encoder {
    pub fn new(ctx: EncodeContext) -> Result<Self, ()> {
        unsafe {
            let mut linesize = Vec::<i32>::new();
            linesize.resize(AV_NUM_DATA_POINTERS as _, 0);
            let mut offset = Vec::<i32>::new();
            offset.resize(AV_NUM_DATA_POINTERS as _, 0);
            let mut length = Vec::<i32>::new();
            length.resize(1, 0);
            let codec = new_encoder(
                CString::new(ctx.name.as_str()).map_err(|_| ())?.as_ptr(),
                ctx.fps,
                ctx.width,
                ctx.height,
                ctx.pixfmt as c_int,
                ctx.align,
                linesize.as_mut_ptr(),
                offset.as_mut_ptr(),
                length.as_mut_ptr(),
                Some(Encoder::callback),
            );

            if codec.is_null() {
                return Err(());
            }

            Ok(Encoder {
                codec: Box::from_raw(codec as *mut c_void),
                frames: Box::into_raw(Box::new(Vec::<EncodeFrame>::new())),
                ctx,
                linesize,
                offset,
                length: length[0],
            })
        }
    }

    pub fn encode(&mut self, data: &[u8]) -> Result<&mut Vec<EncodeFrame>, i32> {
        unsafe {
            (&mut *self.frames).clear();
            let result = encode(
                &mut *self.codec,
                (*data).as_ptr(),
                data.len() as _,
                self.frames as *const _ as *const c_void,
            );
            if result != 0 {
                error!("Error encode: {}", result);
                return Err(result);
            }
            Ok(&mut *self.frames)
        }
    }

    extern "C" fn callback(data: *const u8, size: c_int, pts: i64, obj: *const c_void) {
        unsafe {
            let frames = &mut *(obj as *mut Vec<EncodeFrame>);
            frames.push(EncodeFrame {
                data: slice::from_raw_parts(data, size as _).to_vec(),
                pts,
            });
        }
    }

    pub fn format_from_name(name: String) -> Result<DataFormat, ()> {
        if name.contains("h264") {
            return Ok(H264);
        } else if name.contains("hevc") {
            return Ok(H265);
        }
        Err(())
    }

    pub fn avaliable_encoders(ctx: EncodeContext) -> Vec<CodecInfo> {
        static mut INSTANCE: Vec<CodecInfo> = vec![];
        static mut CACHED_CTX: Option<EncodeContext> = None;

        unsafe {
            if CACHED_CTX.clone().take() != Some(ctx.clone()) {
                CACHED_CTX = Some(ctx.clone());
                INSTANCE = Encoder::avaliable_encoders_(ctx);
            }
            INSTANCE.clone()
        }
    }

    // TODO
    fn avaliable_encoders_(ctx: EncodeContext) -> Vec<CodecInfo> {
        let mut codecs = vec![
            // 264
            CodecInfo {
                name: "h264_nvenc".to_owned(),
                format: H264,
                vendor: NVIDIA,
                score: 92,
                ..Default::default()
            },
            CodecInfo {
                name: "h264_amf".to_owned(),
                format: H264,
                vendor: AMD,
                score: 92,
                ..Default::default()
            },
            CodecInfo {
                name: "h264_qsv".to_owned(), // nv12 only
                format: H264,
                vendor: INTEL,
                score: 70,
                ..Default::default()
            },
            // 265
            CodecInfo {
                name: "hevc_nvenc".to_owned(),
                format: H265,
                vendor: NVIDIA,
                score: 94,
                ..Default::default()
            },
            CodecInfo {
                name: "hevc_amf".to_owned(),
                format: H265,
                vendor: AMD,
                score: 94,
                ..Default::default()
            },
            CodecInfo {
                name: "hevc_qsv".to_owned(), // nv12 only
                format: H265,
                vendor: INTEL,
                score: 60,
                ..Default::default()
            },
        ];

        // qsv doesn't support yuv420p
        codecs.retain(|c| {
            let ctx = ctx.clone();
            if ctx.pixfmt == AVPixelFormat::AV_PIX_FMT_YUV420P && c.name.contains("qsv") {
                return false;
            }
            return true;
        });

        let mut res = vec![];

        if let Ok(yuv) = Encoder::dummy_yuv(ctx.clone()) {
            for codec in codecs {
                let c = EncodeContext {
                    name: codec.name.clone(),
                    ..ctx
                };
                if let Ok(mut encoder) = Encoder::new(c) {
                    if let Ok(_) = encoder.encode(&yuv) {
                        res.push(codec);
                    }
                }
            }
        }

        res
    }

    fn dummy_yuv(ctx: EncodeContext) -> Result<Vec<u8>, ()> {
        let mut yuv = vec![];
        if let Ok((_, _, len)) = ffmpeg_linesize_offset_length(
            ctx.pixfmt,
            ctx.width as _,
            ctx.height as _,
            ctx.align as _,
        ) {
            yuv.resize(len as _, 0);
            return Ok(yuv);
        }

        Err(())
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        unsafe {
            free_encoder(self.codec.as_mut());
            Box::from_raw(self.frames);
            trace!("Encoder dropped");
        }
    }
}
