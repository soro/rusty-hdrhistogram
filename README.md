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

let mut histogram = Histogram::builder()
    .significant_digits(3)
    .highest_trackable_value(60_000)
    .build()
    .unwrap();
histogram.record_value(42).unwrap();
histogram.record_value_with_count(1_000, 3).unwrap();

assert_eq!(histogram.get_total_count(), 4);
assert!(histogram.get_value_at_percentile(99.0) >= 1_000);
```

`Histogram::builder()` builds the usual `u64` integer histogram. The
`.significant_digits(3)` call keeps roughly three decimal significant digits of
precision. Use
`HistogramWithCounter::<u32>::builder().significant_digits(3)` when narrower counters are useful.

## Double Histograms

```rust
use hdrhistogram::DoubleHistogram;

let mut histogram = DoubleHistogram::new(3).unwrap();
histogram.record_value(1.5).unwrap();
histogram.record_value_with_count(10.0, 2).unwrap();

assert_eq!(histogram.get_total_count(), 3);

for value in histogram.recorded_values() {
    assert!(value.value_iterated_to >= 0.0);
}
```

`DoubleHistogram` returns an error when a finite value cannot be represented.
`SaturatingDoubleHistogram` clamps slot selection for finite out-of-range
values. Double histograms use an internal integer histogram and automatically
shift their covered range as values move across orders of magnitude.

## Concurrent Recording

For concurrent writers, prefer recorders over direct concurrent histogram use.
Writers record through a lock-free phaser path, while one reader periodically
takes a locking sample.

```rust
use hdrhistogram::concurrent::recorder;

let recorder = recorder::resizable_with_low_high_sigvdig(1, 60_000, 3).unwrap();

recorder.record_value(100).unwrap();
recorder.record_value(200).unwrap();

let mut sample = recorder.locking_sample();
assert_eq!(sample.snapshot().get_total_count(), 2);

recorder.record_value(300).unwrap();
sample = sample.resample();
assert_eq!(sample.snapshot().get_total_count(), 1);
```

Available recorder constructors include:

```rust
use hdrhistogram::concurrent::recorder;

recorder::single_writer;
recorder::single_writer_with_low_high_sigvdig;
recorder::fixed_with_low_high_sigvdig;
recorder::resizable_with_low_high_sigvdig;
recorder::single_writer_double;
recorder::single_writer_double_with_highest_to_lowest_value_ratio;
recorder::double;
recorder::double_with_highest_to_lowest_value_ratio;
recorder::saturating_double;
recorder::saturating_double_with_highest_to_lowest_value_ratio;
recorder::saturating_single_writer_double;
recorder::saturating_single_writer_double_with_highest_to_lowest_value_ratio;
```

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
`locking_sample().resample()` workflow. Its samples expose a read-only
`ConcurrentDoubleSnapshot`, so sampled interval data can be queried, iterated,
and encoded without exposing recording or reset methods.

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

let histogram = Histogram::builder()
    .significant_digits(3)
    .highest_trackable_value(60_000)
    .build()
    .unwrap();
let capacity = histogram.settings().v2_encoding_capacity();
assert!(capacity > 0);
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

## API Shape

The crate keeps many Java-style method names such as `record_value` and
`get_value_at_percentile` to make HdrHistogram behavior recognizable. The main
types are also re-exported at the crate root for Rust-style imports:

```rust
use hdrhistogram::{
    ConcurrentDoubleHistogram, DoubleHistogram, FixedConcurrentHistogram, Histogram,
    HistogramSettings, HistogramWithCounter, ResizableConcurrentHistogram,
};
```

Lower-level implementation modules such as the phaser, backing arrays, iterator
strategies, and internal construction hooks are hidden. Most applications
should use `Histogram`, `DoubleHistogram`, and `concurrent::recorder`.

## Iteration

Integer histograms, double histograms, recorder samples, concurrent snapshots,
and captured concurrent read views expose percentile, linear, logarithmic,
all-values, and recorded-values iterators:

```rust
use hdrhistogram::Histogram;

let mut histogram = Histogram::builder()
    .significant_digits(3)
    .highest_trackable_value(60_000)
    .build()
    .unwrap();
histogram.record_value(10).unwrap();

for value in histogram.recorded_values() {
    assert!(value.count_at_value_iterated_to > 0);
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
