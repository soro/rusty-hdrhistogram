use byteorder::BigEndian;
use bytes::{BufMut, IntoBuf, BytesMut};
use core::{Counter, ReadableHistogram, SerializationError, SliceableHistogram};
use miniz_oxide::deflate::CompressionLevel;
use serialization::{compression, cookie, Compressor};
use serialization::constants::*;
use serialization::zigzag;

pub trait SerializableBounds<C: Counter> : SliceableHistogram<C> + ReadableHistogram {}
impl<C: Counter, H> SerializableBounds<C> for H where H: SliceableHistogram<C> + ReadableHistogram {}

pub trait SerializableHistogram<C>: ReadableHistogram {
    fn serialize_into<T: BufMut>(&mut self, buf: &mut T) -> Result<usize, SerializationError>;

    fn serialize_into_custom_compressed<T: BufMut, S: Compressor>(
        &mut self,
        buf: &mut T,
        compressor: &mut S,
    ) -> Result<usize, SerializationError>;

    fn allocate_serialization_buffer(&self) -> BytesMut {
        let required_cap = get_required_byte_buffer_capacity(self.array_length());
        BytesMut::with_capacity(required_cap as usize)
    }

    fn required_buffer_capacity(&self) -> usize {
        get_required_byte_buffer_capacity(self.array_length()) as usize
    }

    fn serialize(&mut self) -> Result<(BytesMut, usize), SerializationError> {
        let mut buffer = self.allocate_serialization_buffer();
        let bytes_written = self.serialize_into(&mut buffer)?;
        buffer.truncate(bytes_written);
        Ok((buffer, bytes_written))
    }

    fn serialize_compressed(&mut self) -> Result<(BytesMut, usize), SerializationError> {
        let mut buffer = self.allocate_serialization_buffer();
        let bytes_written = self.serialize_into_compressed(&mut buffer)?;
        buffer.truncate(bytes_written);
        Ok((buffer, bytes_written))
    }

    fn serialize_into_compressed<T: BufMut>(&mut self, buf: &mut T) -> Result<usize, SerializationError> {
        let mut compressor = compression::flate2_compressor(CompressionLevel::DefaultLevel);
        self.serialize_into_custom_compressed(buf, &mut compressor)
    }
}

pub fn get_required_byte_buffer_capacity(length: u32) -> u32 {
    length * 9 + ENCODING_HEADER_SIZE
}

fn fill_buffer_from_counts_slice<T: BufMut, C: Counter>(buf: &mut T, counts_slice: &[C]) -> Result<(), SerializationError> {
    let mut zeroes_count = 0_i64;
    for count in counts_slice.iter() {
        if *count == C::zero() {
            zeroes_count += 1;
        } else {
            if zeroes_count != 0 {
                zigzag::put_i64(buf, -zeroes_count);
                zeroes_count = 0;
            }
            zigzag::put_u64(buf, count.as_u64())?;
        }
    }
    Ok(())
}

impl<C, H> SerializableHistogram<C> for H
where
    C: Counter,
    H: SerializableBounds<C>
{
    fn serialize_into<T: BufMut>(&mut self, buf: &mut T) -> Result<usize, SerializationError> {
        let max_value = self.get_max_value();

        let relevant_length = self.settings().counts_array_index(max_value) + 1;
        if buf.remaining_mut() < get_required_byte_buffer_capacity(relevant_length) as usize {
            return Err(SerializationError::BufferCapacityInsufficient);
        }
        let init_ptr = unsafe { buf.bytes_mut().as_ptr() };

        let settings = self.settings();
        buf.put_u32::<BigEndian>(cookie::get_encoding_cookie());

        let payload_size_ptr = unsafe { buf.bytes_mut().as_ptr() };

        buf.put_u32::<BigEndian>(0);
        buf.put_u32::<BigEndian>(0); // TODO: turn this into normalizing index offset
        buf.put_u32::<BigEndian>(settings.number_of_significant_value_digits);
        buf.put_u64::<BigEndian>(settings.lowest_discernible_value);
        buf.put_u64::<BigEndian>(settings.highest_trackable_value);
        buf.put_f64::<BigEndian>(settings.integer_to_double_value_conversion_ratio);

        let payload_start_ptr = unsafe { buf.bytes_mut().as_ptr() };

        let counts_slice = self.get_counts_slice(relevant_length).unwrap();
        fill_buffer_from_counts_slice(buf, counts_slice)?;

        let payload_end_ptr = unsafe { buf.bytes_mut().as_ptr() };

        unsafe {
            *(payload_size_ptr as *mut u32) = (payload_end_ptr as usize - payload_start_ptr as usize) as u32;
        }

        Ok(payload_end_ptr as usize - init_ptr as usize)
    }

    fn serialize_into_custom_compressed<T: BufMut, S: Compressor>(
        &mut self,
        buf: &mut T,
        compressor: &mut S,
    ) -> Result<usize, SerializationError> {
        let required_capacity = get_required_byte_buffer_capacity(self.array_length()) as usize;
        if buf.remaining_mut() < required_capacity {
            return Err(SerializationError::BufferCapacityInsufficient);
        }

        let mut intermediate_uncompressed = BytesMut::with_capacity(required_capacity);
        let uncompressed_size = self.serialize_into(&mut intermediate_uncompressed)?;
        intermediate_uncompressed.truncate(uncompressed_size);

        buf.put_u32::<BigEndian>(compressor.modify_cookie(cookie::get_compressed_encoding_cookie()));

        let size_ptr = unsafe { buf.bytes_mut().as_ptr() as *mut u32 };

        buf.put_u32::<BigEndian>(0);

        let mut compression_input = (&intermediate_uncompressed).into_buf();

        // TODO: retry with extension if this fails
        let bytes_written = compressor.compress_into(&mut compression_input, buf)?;
        unsafe {
            *size_ptr = bytes_written as u32;
        }

        Ok(bytes_written + 8)
    }
}
