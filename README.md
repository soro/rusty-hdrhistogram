# HdrHistogram-Rust

Rust port of HdrHistogram with integer histograms, double histograms,
concurrent recorders, and Java-compatible encoding/log processing support.

The library, test suite, and Criterion benchmarks build on stable Rust. If you
need a maintained stable Rust port without this recorder implementation, see
https://github.com/HdrHistogram/HdrHistogram_rust.

## Quick Start

```rust
use hdrhistogram::Histogram;

let mut histogram = Histogram::<u64>::with_high_sigvdig(60_000, 3).unwrap();
histogram.record_value(42).unwrap();
histogram.record_value_with_count(1_000, 3).unwrap();

assert_eq!(histogram.get_total_count(), 4);
assert!(histogram.get_value_at_percentile(99.0) >= 1_000);
```

`Histogram<u64>` is the usual integer histogram type. `Histogram<u32>` is also
supported when narrower counters are useful.

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
values. Double histograms expose the same main iterator shapes as integer
histograms: percentile, linear, logarithmic, recorded-value, and all-value.

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
recorder::static_with_low_high_sigvdig;
recorder::resizable_with_low_high_sigvdig;
recorder::single_writer_double;
recorder::single_writer_double_with_highest_to_lowest_value_ratio;
recorder::double;
recorder::double_with_highest_to_lowest_value_ratio;
recorder::saturating_double;
recorder::saturating_double_with_highest_to_lowest_value_ratio;
```

`SingleWriterRecorder` and `SingleWriterDoubleRecorder` use plain histogram
storage behind the recorder swap path. They support one active writer plus a
sampling thread; concurrent writer calls are a contract violation and panic
rather than racing the underlying histogram.

`StaticHistogram` corresponds roughly to Java's `AtomicHistogram`.
`ResizableConcurrentHistogram` corresponds roughly to Java's `ConcurrentHistogram`.
Concurrent histogram scan-style iteration is exposed through recorder samples
and snapshots; direct live concurrent histograms keep point queries and a
captured read view for structurally safe encoding.
The double recorder uses `ConcurrentDoubleHistogram` and the same
`locking_sample().resample()` workflow.

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

let histogram = Histogram::<u64>::with_high_sigvdig(60_000, 3).unwrap();
let capacity = histogram.settings().v2_encoding_capacity();
assert!(capacity > 0);
```

The optional CLI can process Java-compatible histogram logs:

```text
cargo run --features cli --bin hdrhistogram -- log process --input latency.hlog --output latency
cargo run --features cli --bin hdrhistogram -- log tags --input latency.hlog
cargo run --features cli --bin hdrhistogram -- log inspect --input latency.hlog --json
cargo run --features cli --bin hdrhistogram -- log filter --input latency.hlog --tag warmup
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
use hdrhistogram::{DoubleHistogram, Histogram, HistogramSettings};
```

Lower-level implementation modules such as the phaser, backing arrays, iterator
strategies, and internal construction hooks are hidden. Most applications
should use `Histogram`, `DoubleHistogram`, and `concurrent::recorder`.

## Iteration

Integer histograms and concurrent `StaticSnapshot`/`ResizableSnapshot` values
expose percentile, linear, logarithmic, all-values, and recorded-values
iterators:

```rust
use hdrhistogram::Histogram;

let mut histogram = Histogram::<u64>::with_high_sigvdig(60_000, 3).unwrap();
histogram.record_value(10).unwrap();

for value in histogram.recorded_values() {
    assert!(value.count_at_value_iterated_to > 0);
}
```
