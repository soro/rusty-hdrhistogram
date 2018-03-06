use core::HistogramSettings;
use core::errors::*;

#[test]
fn unit_magnitude_0_index_calculations() {
    let s = HistogramSettings::new(1_u64, 1_u64 << 32, 3).unwrap();

    assert_eq!(2048, s.sub_bucket_count);
    assert_eq!(0, s.unit_magnitude);
    assert_eq!(23, s.bucket_count);

    assert_eq!(0, s.get_bucket_index(3));
    assert_eq!(3, s.get_sub_bucket_index(3, 0));

    assert_eq!(0, s.get_bucket_index(1024 + 3));
    assert_eq!(1024 + 3, s.get_sub_bucket_index(1024 + 3, 0));

    assert_eq!(1, s.get_bucket_index(2048 + 3 * 2));
    assert_eq!(1024 + 3, s.get_sub_bucket_index(2048 + 3 * 2, 1));

    assert_eq!(2, s.get_bucket_index((2048 << 1) + 3 * 4));
    assert_eq!(1024 + 3, s.get_sub_bucket_index((2048 << 1) + 3 * 4, 2));

    assert_eq!(23, s.get_bucket_index((2048_u64 << 22) + 3 * (1 << 23)));
    assert_eq!(
        1024 + 3,
        s.get_sub_bucket_index((2048_u64 << 22) + 3 * (1 << 23), 23)
    );
}

#[test]
fn unit_magnitude_4_index_calculations() {
    let s = HistogramSettings::new(1_u64 << 12, 1_u64 << 32, 3).unwrap();

    assert_eq!(2048, s.sub_bucket_count);
    assert_eq!(12, s.unit_magnitude);
    assert_eq!(11, s.bucket_count);
    let unit = 1_u64 << 12;

    assert_eq!(0, s.get_bucket_index(3));
    assert_eq!(0, s.get_sub_bucket_index(3, 0));

    assert_eq!(0, s.get_bucket_index(3 * unit));
    assert_eq!(3, s.get_sub_bucket_index(3 * unit, 0));

    assert_eq!(0, s.get_bucket_index(unit * (1024 + 3)));
    assert_eq!(1024 + 3, s.get_sub_bucket_index(unit * (1024 + 3), 0));

    assert_eq!(1, s.get_bucket_index((unit << 11) + 3 * (unit << 1)));
    assert_eq!(
        1024 + 3,
        s.get_sub_bucket_index((unit << 11) + 3 * (unit << 1), 1)
    );

    assert_eq!(2, s.get_bucket_index((unit << 12) + 3 * (unit << 2)));
    assert_eq!(
        1024 + 3,
        s.get_sub_bucket_index((unit << 12) + 3 * (unit << 2), 2)
    );

    assert_eq!(11, s.get_bucket_index((unit << 21) + 3 * (unit << 11)));
    assert_eq!(
        1024 + 3,
        s.get_sub_bucket_index((unit << 21) + 3 * (unit << 11), 11)
    );
}

#[test]
fn unit_magnitude_52_sub_bucket_magnitude_11_index_calculations() {
    let s = HistogramSettings::new(1_u64 << 52, u64::max_value(), 3).unwrap();

    assert_eq!(2048, s.sub_bucket_count);
    assert_eq!(52, s.unit_magnitude);
    assert_eq!(2, s.bucket_count);
    assert_eq!(1, s.leading_zero_count_base);

    let unit = 1_u64 << 52;

    assert_eq!(0, s.get_bucket_index(3));
    assert_eq!(0, s.get_sub_bucket_index(3, 0));

    assert_eq!(0, s.get_bucket_index(3 * unit));
    assert_eq!(3, s.get_sub_bucket_index(3 * unit, 0));

    assert_eq!(0, s.get_bucket_index(unit * (1024 + 3)));
    assert_eq!(1024 + 3, s.get_sub_bucket_index(unit * (1024 + 3), 0));

    assert_eq!(0, s.get_bucket_index(unit * 1024 + 1023 * unit));
    assert_eq!(
        1024 + 1023,
        s.get_sub_bucket_index(unit * 1024 + 1023 * unit, 0)
    );

    assert_eq!(1, s.get_bucket_index((unit << 11) + 3 * (unit << 1)));
    assert_eq!(
        1024 + 3,
        s.get_sub_bucket_index((unit << 11) + 3 * (unit << 1), 1)
    );

    assert_eq!(1, s.get_bucket_index(u64::max_value()));
    assert_eq!(1024 + 1023, s.get_sub_bucket_index(u64::max_value(), 1));
}

#[test]
fn unit_magnitude_53_sub_bucket_magnitude_11_throws() {
    let res = match HistogramSettings::new(1_u64 << 53, 1 << 63, 3) {
        Err(e) => e,
        _ => panic!("precise time doesn't implement debug xd"),
    };
    assert_eq!(CreationError::CantReprSigDigitsLtLowestDiscernible, res);
}

#[test]
fn unit_magnitude_55_sub_bucket_magnitude_8_ok() {
    let s = HistogramSettings::new(1_u64 << 55, 1 << 63, 2).unwrap();

    assert_eq!(256, s.sub_bucket_count);
    assert_eq!(55, s.unit_magnitude);
    assert_eq!(2, s.bucket_count);
    assert_eq!(0, s.get_bucket_index(3));
    assert_eq!(0, s.get_sub_bucket_index(3, 0));
    assert_eq!(1, s.get_bucket_index(u64::max_value()));
    assert_eq!(128 + 127, s.get_sub_bucket_index(u64::max_value(), 1));
}

#[test]
fn unit_magnitude_62_sub_bucket_magnitude_1_ok() {
    let s = HistogramSettings::new(1_u64 << 62, 1 << 63, 0).unwrap();

    assert_eq!(2, s.sub_bucket_count);
    assert_eq!(62, s.unit_magnitude);
    assert_eq!(2, s.bucket_count);
    assert_eq!(0, s.get_bucket_index(3));
    assert_eq!(0, s.get_sub_bucket_index(3, 0));
    assert_eq!(1, s.get_bucket_index(u64::max_value()));
    assert_eq!(1, s.get_sub_bucket_index(u64::max_value(), 1));
}
