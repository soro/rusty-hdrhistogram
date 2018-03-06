use bytes::{Buf, BufMut};
use core::*;

macro_rules! shift {
    ($value:expr, $factor:expr) => { ($value >> (7 * $factor)) as u8 };
}

macro_rules! shift_sign {
    ($value:expr, $factor:expr) => { (($value >> (7 * $factor)) | 0x80) as u8 };
}

macro_rules! put_chunks {
    // terminal case
    ($buf:expr, $value:expr, $factor:expr, ) => {
        if $value >> (7 * ($factor + 1)) == 0 {
            $buf.put_u8(shift!($value, $factor));
        }
        else {
            $buf.put_u8(shift_sign!($value, $factor));
            $buf.put_u8(shift!($value, $factor + 1));
        }
    };
    // TODO: remove the fuel head splitting here?
    // base case
    ($buf:expr, $value:expr, 0, $fuel_head:tt $($fuel:tt) *) => {
        if $value >> 7 == 0 {
            $buf.put_u8($value as u8);
        }
        else {
            $buf.put_u8((($value & 0x7F) | 0x80) as u8);
            put_chunks!($buf, $value, 1, $($fuel) *);
        }
    };
    ($buf:expr, $value:expr, $factor:expr, $fuel_head:tt $($fuel:tt) *) => {
        if $value >> (7 * ($factor + 1)) == 0 {
            $buf.put_u8(shift!($value, $factor));
        }
        else {
            $buf.put_u8(shift_sign!($value, $factor));
            put_chunks!($buf, $value, $factor + 1, $($fuel) *);
        }
    };
}


pub fn put_u64<T: BufMut>(buffer: &mut T, value: u64) -> Result<(), SerializationError> {
    if value >> 63 == 1 {
        Err(SerializationError::ValueNotLEBEncodable)
    } else {
        Ok(put_i64(buffer, value as i64))
    }
}

pub fn put_i64<T: BufMut>(buffer: &mut T, value: i64) {
    let value = ((value << 1) ^ (value >> 63)) as i64;
    put_chunks!(buffer, value, 0, a a a a a a a);
}

pub fn put_u32<T: BufMut>(buffer: &mut T, value: u32) -> Result<(), SerializationError> {
    if value >> 31 == 1 {
        Err(SerializationError::ValueNotLEBEncodable)
    } else {
        Ok(put_i32(buffer, value as i32))
    }
}

pub fn put_i32<T: BufMut>(buffer: &mut T, value: i32) {
    let value = ((value << 1) ^ (value >> 31)) as i32;
    put_chunks!(buffer, value, 0, a a a);
}

macro_rules! read_chunks {
    ($buf:expr, $value:expr, $byte:expr, $factor:expr, $tpe:ty, ) => {
        $byte = $buf.get_u8();
        $value |= ($byte as $tpe) << (7 * $factor);
    };
    ($buf:expr, $value:expr, $byte:expr, $factor:expr, $tpe:ty, $fuel_head:tt $($fuel:tt) *) => {
        $byte = $buf.get_u8();
        $value |= (($byte & 0x7F) as $tpe) << (7 * $factor);
        if ($byte & 0x80) != 0 {
            read_chunks!($buf, $value, $byte, $factor + 1, $tpe, $($fuel) *);
        }
    }
}

pub fn get_i64<T: Buf>(buffer: &mut T) -> i64 {
    let mut v = buffer.get_u8();
    let mut value = (v & 0x7F) as u64;
    if (v & 0x80) != 0 {
        read_chunks!(buffer, value, v, 1, u64, a a a a a a a);
    }
    (((value >> 1) as i64) ^ -((value & 1) as i64))
}

pub fn get_i32<T: Buf>(mut buffer: T) -> i32 {
    let mut v = buffer.get_u8();
    let mut value = (v & 0x7F) as u32;
    if (v & 0x80) != 0 {
        read_chunks!(buffer, value, v, 1, u32, a a a);
    }
    ((value >> 1) as i32) ^ -((value & 1) as i32)
}
