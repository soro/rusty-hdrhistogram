use byteorder::BigEndian;
use bytes::{Buf, BytesMut, IntoBuf};
use core::{ConstructableHistogram, Counter, DeserializationError, MutSliceableHistogram, ReadableHistogram};
use serialization::compression::{self, Decompressor};
use serialization::zigzag;

pub trait DeserializableBounds<C: Counter> : MutSliceableHistogram<C> + ReadableHistogram + ConstructableHistogram {}
impl<C: Counter, H> DeserializableBounds<C> for H where H: MutSliceableHistogram<C> + ReadableHistogram + ConstructableHistogram {}

pub trait DeserializableHistogram<C>: Sized {
    fn deserialize_from<T: Buf>(buf: &mut T, min_highest_trackable_value: C) -> Result<Self, DeserializationError>;

    fn deserialize_from_compressed<T: Buf>(buf: &mut T, min_highest_trackable_value: C) -> Result<Self, DeserializationError> {
        let decompressor = compression::flate2_decompressor();
        Self::deserialize_from_custom_compressed(buf, min_highest_trackable_value, decompressor)
    }

    fn deserialize_from_custom_compressed<T: Buf, S: Decompressor>(
        buf: &mut T,
        min_highest_trackable_value: C,
        mut decompressor: S,
    ) -> Result<Self, DeserializationError> {
        let cookie = buf.get_u32::<BigEndian>();
        let length_of_compressed = buf.get_u32::<BigEndian>();

        let should_decompress = decompressor.check_cookie(cookie)?;

        if should_decompress {
            let mut decompression_buf_cap = get_decompression_buf_capacity(length_of_compressed);
            let mut decompression_buffer = BytesMut::with_capacity(decompression_buf_cap);
            let mut bytes_written = 0;
            loop {
                match decompressor.decompress_from(buf, &mut decompression_buffer) {
                    Ok(bytes_out) => {
                        bytes_written += bytes_out;
                        decompression_buffer.truncate(bytes_written);
                        let mut as_buf = decompression_buffer.freeze().into_buf();

                        return Self::deserialize_from(&mut as_buf, min_highest_trackable_value);
                    }
                    Err(DeserializationError::DecompressionBufferCapacityInsufficient(_, bytes_out)) => {
                        decompression_buffer.reserve(decompression_buf_cap);
                        decompression_buf_cap <<= 1;
                        bytes_written += bytes_out;
                    }
                    Err(e) => {
                        return Err(e);
                    }
                }
            }
        } else {
            Self::deserialize_from(buf, min_highest_trackable_value)
        }
    }
}

fn get_decompression_buf_capacity(length_of_compressed: u32) -> usize {
    let mut cap = 1;
    while cap < 2 * length_of_compressed {
        cap <<= 1
    }
    cap as usize
}

fn fill_counts_slice_from_buffer<T: Buf, C: Counter>(
    slice: &mut [C],
    mut buf: T,
    counts_array_length: u32,
) -> Result<(), DeserializationError> {
    let mut idx = 0_u32;
    while buf.has_remaining() && idx < counts_array_length {
        let count = zigzag::get_i64(&mut buf);
        if count < 0 {
            idx += (-count) as u32;
        } else {
            let count = C::from_i64(count).ok_or(DeserializationError::CountExceedsTypeMax)?;
            unsafe { *slice.get_unchecked_mut(idx as usize) = count };
            idx += 1;
        }
    }
    if buf.has_remaining() {
        Err(DeserializationError::PayloadExceededCountsArrayLength)
    } else {
        Ok(())
    }
}

impl<C, H> DeserializableHistogram<C> for H
where
    C: Counter,
    H: DeserializableBounds<C>
{
    fn deserialize_from<T: Buf>(
        buf: &mut T,
        min_highest_trackable_value: C,
    ) -> Result<Self, DeserializationError> {
        let _ = buf.get_u32::<BigEndian>();
        let _ = buf.get_u32::<BigEndian>();
        let normalizing_index_offset = buf.get_u32::<BigEndian>();
        let number_of_significant_value_digits = buf.get_u32::<BigEndian>();
        let lowest_trackable_unit_value = buf.get_u64::<BigEndian>();
        let highest_trackable_value = buf.get_u64::<BigEndian>();
        let integer_to_double_value_conversion_ratio = buf.get_f64::<BigEndian>();

        let highest_trackable = highest_trackable_value.max(min_highest_trackable_value.as_u64());

        let mut histogram = Self::new(
            lowest_trackable_unit_value,
            highest_trackable,
            number_of_significant_value_digits as u8,
        ).map_err(|e| DeserializationError::HistogramCreationFailed(e))?;
        let counts_array_length = histogram.array_length();
        // TODO:
        //        histogram.setIntegerToDoubleValueConversionRatio(integerToDoubleValueConversionRatio);
        //        histogram.setNormalizingIndexOffset(normalizingIndexOffset);

        {
            let mut counts_slice = histogram.get_counts_slice_mut(counts_array_length).unwrap();
            fill_counts_slice_from_buffer(&mut counts_slice, buf, counts_array_length)?;
        }

        histogram.establish_internal_tracking_values();

        Ok(histogram)
    }
}
