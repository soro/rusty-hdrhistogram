use bytes::{Buf, BufMut};
use core::{DeserializationError, SerializationError};
use miniz_oxide::deflate::CompressionLevel;
use miniz_oxide::deflate::core::{compress, create_comp_flags_from_zip_params, CompressorOxide, TDEFLFlush, TDEFLStatus};
use miniz_oxide::inflate::TINFLStatus;
use miniz_oxide::inflate::core::{decompress, DecompressorOxide};
use miniz_oxide::inflate::core::inflate_flags;
use serialization::constants::*;
use serialization::cookie;
use std::io::Cursor;

pub trait Compressor {
    fn compress_into<T: Buf, S: BufMut>(&mut self, src_buffer: &mut T, tgt_buffer: &mut S) -> Result<usize, SerializationError>;
    fn modify_cookie(&self, cookie: u32) -> u32;
}

pub trait Decompressor {
    fn decompress_from<T: Buf, S: BufMut>(&mut self, src_buffer: &mut T, tgt_buffer: &mut S) -> Result<usize, DeserializationError>;
    fn check_cookie(&self, cookie: u32) -> Result<bool, DeserializationError>;
}

pub fn flate2_compressor(level: CompressionLevel) -> CompressorOxide {
    let flags = create_comp_flags_from_zip_params(level as i32, 1, 0);
    CompressorOxide::new(flags)
}

pub fn flate2_decompressor() -> DecompressorOxide {
    DecompressorOxide::new()
}

impl Compressor for CompressorOxide {
    fn compress_into<T: Buf, S: BufMut>(&mut self, src_buffer: &mut T, tgt_buffer: &mut S) -> Result<usize, SerializationError> {
        unsafe {
            let (status, _, bytes_out) = compress(
                self,
                src_buffer.bytes(),
                tgt_buffer.bytes_mut(),
                TDEFLFlush::Finish,
            );
            if status == TDEFLStatus::PutBufFailed {
                Err(SerializationError::CompressedBufferCapacityInsufficient)
            } else {
                tgt_buffer.advance_mut(bytes_out);
                Ok(bytes_out)
            }
        }
    }
    fn modify_cookie(&self, cookie: u32) -> u32 {
        cookie
    }
}

const DECOMPRESS_FLAGS: u32 = inflate_flags::TINFL_FLAG_PARSE_ZLIB_HEADER | inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF;


impl Decompressor for DecompressorOxide {
    fn decompress_from<T: Buf, S: BufMut>(&mut self, src_buffer: &mut T, tgt_buffer: &mut S) -> Result<usize, DeserializationError> {
        let (status, bytes_in, bytes_out) = {
            let tgt_bytes = unsafe { tgt_buffer.bytes_mut() };
            let mut out_cursor = Cursor::new(tgt_bytes);
            decompress(self, src_buffer.bytes(), &mut out_cursor, DECOMPRESS_FLAGS)
        };
        src_buffer.advance(bytes_in);
        unsafe { tgt_buffer.advance_mut(bytes_out) };
        if status == TINFLStatus::Done {
            Ok(bytes_out)
        } else if status == TINFLStatus::HasMoreOutput {
            Err(DeserializationError::DecompressionBufferCapacityInsufficient(bytes_in, bytes_out))
        } else {
            Err(DeserializationError::DecompressionFailed)
        }
    }

    fn check_cookie(&self, cookie: u32) -> Result<bool, DeserializationError> {
        let cookie_base = cookie::get_cookie_base(cookie);
        match cookie_base {
            V2_ENCODING_COOKIE_BASE => Ok(false),
            V2_COMPRESSED_ENCODING_COOKIE_BASE => Ok(true),
            _ => Err(DeserializationError::CompressionFormatNotRecognized),
        }
    }
}
