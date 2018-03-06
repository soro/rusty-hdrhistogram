use serialization::constants::*;

pub fn get_encoding_cookie() -> u32 {
    V2_ENCODING_COOKIE_BASE | 0x10
}

pub fn get_compressed_encoding_cookie() -> u32 {
    V2_COMPRESSED_ENCODING_COOKIE_BASE | 0x10
}

pub fn get_cookie_base(cookie: u32) -> u32 {
    cookie & !0xf0
}
