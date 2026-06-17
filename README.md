# HdrHistogram-Rust

This is a Rust port of HdrHistogram with integer histograms, double histograms,
concurrent recorders, and Java-compatible encoding/log processing support.

I first wrote the largest part of this implementation in 2018 by hand, but ended up not
having the time to really support and extend the library, which wasn't really a priority
for me at the time, since another port existed and I mostly wrote this to learn Rust.
I have now spent some time completing various other parts and overhauling the codebase
using AI tooling, with plenty of careful review.

The Rust versions are consistently as fast or faster than the Java originals, with
slightly improved guarantees in one or two cases. The API is of course slightly
different, as it has to leverage some Rust RAII patterns to be ergonomic and safe.

The library, test suite, and Criterion benchmarks build on stable Rust.

## Quick Start

```rust
use hdrhistogram::Histogram;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut histogram = Histogram::builder()
        .significant_digits(3)
        .highest_trackable_value(60_000)
        .build()?;
    histogram.record_value(42)?;
    histogram.record_value_with_count(1_000, 3)?;

    assert_eq!(histogram.get_total_count(), 4);
    assert!(histogram.get_value_at_percentile(99.0) >= 1_000);

    Ok(())
}
```

`Histogram::builder()` builds the usual `u64` integer histogram. The
`.significant_digits(3)` call keeps roughly three decimal significant digits of
precision. Use
`hdrhistogram::st::HistogramWithCounter::<u32>::builder().significant_digits(3)` when narrower counters are useful.

## Fast Fixed-Range Histograms

For the lowest single-threaded recording cost, define a static fixed-range
histogram when the value range is known ahead of time. Static histograms store
the count array directly in the histogram object and encode the HdrHistogram
layout in the type, so recording avoids both the `Vec` allocation and per-value
layout field loads used by the resizable `Histogram`.

```rust
hdrhistogram::static_histogram! {
    type RequestLatencyHistogram = {
        lowest_discernible_value: 1,
        highest_trackable_value: 3_600 * 1_000 * 1_000,
        significant_digits: 3,
    };
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut histogram = RequestLatencyHistogram::new();
    histogram.record_value(42)?;

    Ok(())
}
```

The `static_histogram!` macro computes the required inline count-array length at
compile time and aliases a fully parameterized `st::StaticHistogram`. Invalid
layout parameters fail at compile time. For very large ranges, remember that the
count array is embedded in the value itself; keep large instances boxed or
stored in long-lived owner structs rather than casually copying them around.

## Double Histograms

```rust
use hdrhistogram::DoubleHistogram;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut histogram = DoubleHistogram::builder()
        .significant_digits(3)
        .build()?;
    histogram.record_value(1.5)?;
    histogram.record_value_with_count(10.0, 2)?;

    assert_eq!(histogram.get_total_count(), 3);

    for value in histogram.recorded_values() {
        assert!(value.value_iterated_to >= 0.0);
    }

    Ok(())
}
```

`DoubleHistogram` returns an error when a finite value cannot be represented.
`SaturatingDoubleHistogram` clamps slot selection for finite out-of-range
values. Double histograms use an internal integer histogram and automatically
shift their covered range as values move across orders of magnitude.

## Concurrent Recording

For concurrent writers, prefer recorders over direct concurrent histogram use.
Writers record through a lock-free phaser path, while one reader periodically
begins an interval sample.

```rust
use hdrhistogram::ResizableRecorder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let recorder = ResizableRecorder::builder()
        .lowest_discernible_value(1)
        .highest_trackable_value(60_000)
        .significant_digits(3)
        .build()?;

    recorder.record_value(100)?;
    recorder.record_value(200)?;

    let mut sample = recorder.begin_interval_sample();
    assert_eq!(sample.snapshot().get_total_count(), 2);

    recorder.record_value(300)?;
    sample = sample.resample();
    assert_eq!(sample.snapshot().get_total_count(), 1);

    Ok(())
}
```

Available recorder builders include:

- `FixedRecorder::builder()`
- `ResizableRecorder::builder()`
- `SingleWriterRecorder::builder()`
- `DoubleRecorder::builder()`
- `SaturatingDoubleRecorder::builder()`
- `SingleWriterDoubleRecorder::builder()`
- `SaturatingSingleWriterDoubleRecorder::builder()`

`SingleWriterRecorder` and `SingleWriterDoubleRecorder` use plain histogram
storage behind the recorder swap path. They support one active writer plus a
sampling thread; concurrent writer calls are a contract violation and panic
rather than racing the underlying histogram.

`FixedConcurrentHistogram` corresponds roughly to Java's `AtomicHistogram`.
`ResizableConcurrentHistogram` corresponds roughly to Java's `ConcurrentHistogram`.
`ConcurrentDoubleHistogram` is the concurrent double-valued variant.
Concurrent histogram scan-style iteration is exposed through recorder samples,
snapshots, and captured read views rather than direct live histogram iterators.
The double recorder uses `ConcurrentDoubleHistogram` and the same
`begin_interval_sample().resample()` workflow. Its samples expose a read-only
`ConcurrentDoubleSnapshot`, so sampled interval data can be queried, iterated,
and encoded without exposing recording or reset methods.

Direct concurrent histogram types live under `hdrhistogram::concurrent`:

```rust
use hdrhistogram::concurrent::ConcurrentDoubleHistogram;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let histogram = ConcurrentDoubleHistogram::builder()
        .significant_digits(3)
        .highest_to_lowest_value_ratio(1_024)
        .build()?;

    histogram.record_value(42.0)?;
    assert_eq!(histogram.get_total_count(), 1);

    Ok(())
}
```

Only one interval sample may be active for a recorder at a time. Holding an
interval sample does not stop writers from recording into the next interval;
resampling may wait for writer calls that were already in flight.

Direct concurrent histograms are intended for shared recording and short-lived
captured read views. Reset-style maintenance is an exclusive operation;
`ConcurrentDoubleHistogram::reset` requires `&mut self`. Use recorders for
reset-after-scrape or interval-sampling workflows.

## Usage Notes

Captured concurrent read views keep backing-array storage and range metadata
stable while the view is alive. That means a long-lived read view can delay
structural changes such as resize or double-histogram range shifts. Keep read
views short-lived, and prefer recorder samples when a stable interval snapshot
is needed.

Captured concurrent read views are structurally stable, but they are not frozen
count snapshots. Writers may continue to update count cells while a view is
alive. Use recorder samples when the encoded or iterated data must represent a
stable interval.

Live concurrent read-view iterators are fallible: use `try_next()` and handle
`IterationError::ConcurrentModification` if writers record after the view is
captured.

## Encoding And Logs

The `encoding` module contains Java-compatible V2 binary encoding, compressed
encoding, Base64 helpers, double-histogram framing, histogram log line helpers,
streaming `HistogramLogReader`/`HistogramLogWriter` building blocks, and
histogram log report generation. `HistogramLogScanner` exposes log metadata
without decoding compressed histogram payloads, which is useful for tag
listing, filtering, and inspection.

Feature flags:

- `encoding-compression`: enables compressed V2 encoding.
- `encoding-base64`: enables Base64 helpers, histogram log helpers, report
  generation, and also enables compression.
- `cli`: builds the optional `hdrhistogram` command-line tool and enables
  `encoding-base64`.
- `loom-tests`: enables opt-in loom model tests for the phaser/recorder
  publication protocol.

Capacity planning for V2 encoding lives on `HistogramSettings`:

```rust
use hdrhistogram::Histogram;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let histogram = Histogram::builder()
        .significant_digits(3)
        .highest_trackable_value(60_000)
        .build()?;
    let capacity = histogram.settings().v2_encoding_capacity();
    assert!(capacity > 0);

    Ok(())
}
```

The optional CLI can process Java-compatible histogram logs:

```text
cargo run --features cli --bin hdrhistogram -- log process --input latency.hlog --output latency
cargo run --features cli --bin hdrhistogram -- log tags --input latency.hlog
cargo run --features cli --bin hdrhistogram -- log summary --input latency.hlog --json
cargo run --features cli --bin hdrhistogram -- log inspect --input latency.hlog --json
cargo run --features cli --bin hdrhistogram -- log filter --input latency.hlog --tag warmup --output warmup.hlog
```

Java-style `HistogramLogProcessor` arguments are also accepted at the top level:

```text
cargo run --features cli --bin hdrhistogram -- -csv -i latency.hlog -o latency -outputValueUnitRatio 1000000
```

## Java Compatibility Notes

The V2 binary encoding and histogram-log formats are intended to be compatible
with Java HdrHistogram. A few edge semantics are worth calling out explicitly:

- Rust histograms use exact integer power-of-two layout math for
  `lowest_discernible_value`. Java HdrHistogram computes the same internal unit
  magnitude through floating-point logarithms, which can round very large
  values just below powers of two into a coarser bucket layout. For typical
  settings such as `lowest_discernible_value: 1`, `1000`, or exact powers of
  two, the layouts match. If you encode histograms with pathological high lower
  bounds such as `2^49 - 1`, prefer this crate's reader and CLI for analysis, or
  choose an exact power-of-two lower bound when Java tooling must consume the
  data.
- Rust double histograms preserve the source `auto_resize` setting in
  `copy_corrected_for_coordinated_omission()`. Java currently creates the
  corrected double-histogram target with auto-resize disabled. Fixed-range Rust
  copies remain fixed and can still return an out-of-range error; auto-resizing
  Rust copies can shift/grow while replaying corrected values.
- Rust avoids a Java double-histogram range-shift bug where the high-side shift
  path can scale the published range twice. This preserves the pre-shift value
  mapping instead of matching Java's current corrupting edge behavior.
- HdrHistogram allocates count storage in bucket-sized chunks, so the allocated
  array can sometimes record values above the configured
  `highest_trackable_value`. Calling `resize(x)` with a value that already fits
  in the current allocation may be treated as a fast no-op; `settings()` and
  encoded headers can continue to report the earlier configured high value even
  though the backing storage can represent `x`.
- Zero-count records follow the Java-style mutation path. Calling
  `record_value_with_count(value, 0)` can update range/min/max tracking even
  though the total count does not increase.
- A zero-only histogram with `lowest_discernible_value > 1` can report the
  highest equivalent value of zero as max after decode or metadata
  recalculation, matching Java's edge behavior. Avoid relying on max-value
  metadata for zero-only histograms with coarse lowest-discernible values.
- Bucket counts, total counts, equivalent-value helpers, and iterator aggregate
  math use unchecked HdrHistogram-style arithmetic for speed. Histograms near
  the top `u64` bucket or with extreme `value * count` products can overflow
  helper calculations, which may panic in debug builds.

## API Shape

The crate keeps many Java-style method names such as `record_value` and
`get_value_at_percentile` to make HdrHistogram behavior recognizable. Common
types are re-exported at the crate root for Rust-style imports:

```rust
use hdrhistogram::{
    DoubleHistogram, DoubleRecorder, FixedRecorder, Histogram, HistogramSettings,
    ResizableRecorder, SingleWriterRecorder,
};
```

Lower-level implementation modules such as the phaser, backing arrays, iterator
strategies, and internal construction hooks are hidden. Most applications
should use `Histogram`, `DoubleHistogram`, and the recorder types. Concurrent
histogram, builder, policy, snapshot, and read-view types remain available from
the `st` and `concurrent` modules when needed.

## Iteration

Integer histograms, double histograms, recorder samples, concurrent snapshots,
and captured concurrent read views expose percentile, linear, logarithmic,
all-values, and recorded-values iterators:

```rust
use hdrhistogram::Histogram;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut histogram = Histogram::builder()
        .significant_digits(3)
        .highest_trackable_value(60_000)
        .build()?;
    histogram.record_value(10)?;

    for value in histogram.recorded_values() {
        assert!(value.count_at_value_iterated_to > 0);
    }

    Ok(())
}
```

Direct live concurrent histograms intentionally do not expose infallible scan
iterators. Take a recorder sample or captured read view first.

## Performance Snapshot

These are example single-thread Criterion timings from this machine, meant as
ballpark guidance rather than a portability guarantee:

- CPU: AMD Ryzen 7 6800H with Radeon Graphics
- Linux CPU governor: `performance`
- Command shape: `cargo bench --bench record -- <record-path-filter>`
- Criterion settings: 30 samples, 1 second warm-up, 3 seconds measurement

| Benchmark | Middle estimate |
| --- | ---: |
| `Histogram::record_value` | 2.46 ns |
| `FixedConcurrentHistogram::record_value` | 3.48 ns |
| `SingleWriterRecorder::record_value` | 3.37 ns |
| `ResizableConcurrentHistogram::record_value` | 6.94 ns |
| `FixedRecorder::record_value` | 6.99 ns |
| `ResizableRecorder::record_value` | 10.56 ns |
| `DoubleHistogram::record_value` | 4.75 ns |
| `ConcurrentDoubleHistogram::record_value` | 7.18 ns |
| `SingleWriterDoubleRecorder::record_value` | 5.39 ns |
| `DoubleRecorder::record_value` | 10.58 ns |

Recorder paths trade a little more writer-side overhead for cheap interval
sampling. The single-writer recorders are intended for the common case where one
thread owns recording and another thread samples.
