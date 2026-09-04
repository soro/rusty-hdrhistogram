# Single-writer recorder split and phaser refactor

## Status

Implemented. This document records the intended design and validation criteria.

## Summary

Refactor the integer and double single-writer recorders into two handles created
together by their builders:

- a unique writer/recorder handle; and
- a unique sampler handle that owns the inactive histogram.

The writer API will require `&mut self`, making the single-writer invariant a
safe Rust ownership property instead of a runtime CAS check. Writer/sampler
handoff will then use the existing writer-reader phaser in the same double-buffer
shape as the Java recorders. The current `SingleWriterCore` state machine,
settings caches, and long-lived phaser sample guard can be removed.

As part of the same public-API refactor, rename all exported `*WithPolicy`
concrete types to the base type name with a default `ThrowOnOverflow` generic
parameter.

## Goals

- Enforce exactly one writer through ownership and `&mut self`.
- Keep concurrent sampling safe without blocking new writer calls during a
  sample handoff.
- Use the phaser, rather than a separate sampling CAS, for writer/sampler
  coordination.
- Allocate both histogram buffers before the writer and sampler can run
  concurrently.
- Remove cached histogram construction settings from the single-writer
  recorders.
- Recycle sampled histograms rather than freeing and reconstructing them between
  sampling sessions.
- Preserve counts, interval boundaries, metadata, range mappings, and overflow
  policy behavior.
- Measure and explicitly accept or reject the writer-side performance change.
- Simplify the policy-bearing public type names.

## Non-goals

- Do not change `FixedRecorder`, `ResizableRecorder`, or the multi-writer
  `DoubleRecorder` handoff algorithm beyond the policy-type rename.
- Do not change ordinary histogram recording paths for the recorder refactor.
- Do not make two recycled buffers maintain identical capacity after every
  independent auto-resize. Each buffer retains its own expanded capacity and
  may expand when it becomes active.
- Do not preserve the current shifted double range across `reset()`. Java and
  the current Rust implementation reset the double range to its initial
  `2^800` position while retaining the configured ratio and backing capacity.
- Do not add compatibility aliases for the old `*WithPolicy` names unless a
  release-policy decision later requires a deprecation window.

## Current architecture

The public recorders currently map to internal coordination as follows:

| Public type | Histogram storage | Coordination |
| --- | --- | --- |
| `FixedRecorder` | `FixedConcurrentHistogram` | `Recorder<T>` and phaser |
| `ResizableRecorder` | `ResizableConcurrentHistogram` | `Recorder<T>` and phaser |
| `DoubleRecorderWithPolicy<P>` | `ConcurrentDoubleHistogramWithPolicy<P>` | `RecorderCore<T>` and phaser |
| `SingleWriterRecorder` | plain `Histogram` | `SingleWriterCore<T>` CAS plus phaser shell |
| `SingleWriterDoubleRecorderWithPolicy<P>` | plain `DoubleHistogramWithPolicy<P>` | `SingleWriterCore<T>` CAS plus phaser shell |

For the single-writer variants, `begin_sampling()` waits for `RECORDING` to
finish, changes the state to `SAMPLING`, prevents new recording calls, swaps the
active pointer, flips a phaser that has no registered single-writer recording
operations, and then returns to `IDLE`.

The phaser currently contributes only its reader mutex/sample lease to the
single-writer variants. Its writer epochs do not protect those recording calls.

Fresh inactive histograms are allocated before `begin_sampling()` so allocation
does not extend the writer exclusion window. This requires construction settings
to be cached in the recorder. Auto-resizing can make the cached capacity or
double highest-to-lowest ratio stale.

## Target public API

### Integer single-writer recorder

`build()` returns both roles directly:

```rust
let (mut recorder, mut sampler) = SingleWriterRecorder::builder()
    .significant_digits(3)
    .auto_resize(true)
    .build()?;

recorder.record_value(42)?;
let mut sample = sampler.begin_interval_sample();
consume(sample.snapshot());
sample = sample.resample();
```

The intended signatures are:

```rust
impl SingleWriterRecorderBuilder {
    pub fn build(
        self,
    ) -> Result<(SingleWriterRecorder, SingleWriterSampler), CreationError>;
}

impl SingleWriterRecorder {
    pub fn record_value(&mut self, value: u64) -> Result<(), RecordError>;
    pub fn record_value_with_count(
        &mut self,
        value: u64,
        count: u64,
    ) -> Result<(), RecordError>;
    pub fn record_value_with_expected_interval(
        &mut self,
        value: u64,
        expected_interval_between_value_samples: u64,
    ) -> Result<(), RecordError>;
}

impl SingleWriterSampler {
    pub fn begin_interval_sample(
        &mut self,
    ) -> SingleWriterIntervalSample<'_>;
}
```

### Double single-writer recorder

```rust
let (mut recorder, mut sampler) = SingleWriterDoubleRecorder::builder()
    .highest_to_lowest_value_ratio(1_024)
    .significant_digits(3)
    .auto_resize(true)
    .build()?;

recorder.record_value(42.0)?;
let mut sample = sampler.begin_interval_sample();
consume(sample.snapshot());
sample = sample.resample();
```

The policy-generic signatures are:

```rust
pub struct SingleWriterDoubleRecorder<
    P: OverflowPolicy = ThrowOnOverflow,
> { /* ... */ }

pub struct SingleWriterDoubleSampler<
    P: OverflowPolicy = ThrowOnOverflow,
> { /* ... */ }

impl<P: OverflowPolicy> SingleWriterDoubleRecorderBuilder<P> {
    pub fn build(
        self,
    ) -> Result<
        (
            SingleWriterDoubleRecorder<P>,
            SingleWriterDoubleSampler<P>,
        ),
        DoubleCreationError,
    >;
}
```

Keep the convenient saturating aliases:

```rust
pub type SaturatingSingleWriterDoubleRecorder =
    SingleWriterDoubleRecorder<SaturateOnOverflow>;

pub type SaturatingSingleWriterDoubleSampler =
    SingleWriterDoubleSampler<SaturateOnOverflow>;
```

### Construction from an existing histogram

`from_histogram()` should follow the builder and return the pair:

```rust
let (mut recorder, mut sampler) =
    SingleWriterRecorder::from_histogram(histogram)?;

let (mut recorder, mut sampler) =
    SingleWriterDoubleRecorder::from_histogram(double_histogram)?;
```

Because the source histogram is passed by value and has not yet been published
to either handle, construction can safely create an empty inactive histogram
with the same current internal model. No concurrent live-histogram inspection is
needed.

An internal `empty_like_for_recorder()` helper should preserve:

- integer layout and current backing capacity;
- significant digits;
- auto-resize behavior;
- integer-to-double conversion metadata where applicable; and
- for double histograms, the current configured highest-to-lowest ratio.

It should not copy counts. The inactive double histogram should use normal
reset range semantics before it first becomes active.

## Target internal ownership

Reuse `RecorderCore<T>` behind `Arc` rather than introducing another pointer and
phaser implementation:

```rust
struct RecorderCore<T> {
    recording_phaser: WriterReaderPhaser,
    active_histogram: AtomicPtr<T>,
}

pub struct SingleWriterRecorder {
    core: Arc<RecorderCore<Histogram>>,
}

pub struct SingleWriterSampler {
    core: Arc<RecorderCore<Histogram>>,
    inactive_histogram: Option<Box<Histogram>>,
}
```

The double types use the same structure parameterized by
`DoubleHistogram<P>`.

Only one writer handle and one sampler handle are constructed. Neither handle
is `Clone`. Recording requires `&mut self`, and beginning a sample requires
`&mut self`, so safe Rust cannot overlap two writers or two sample operations.
The handles may be moved independently to different threads.

`RecorderCore<T>` logically owns the histogram currently stored in
`active_histogram`. The sampler or its active interval sample owns the other
histogram as a `Box<T>`.

## Phaser handoff algorithm

### Recording

Every complete logical recording operation, including coordinated-omission
correction, stays within one phaser writer critical section:

```rust
pub fn record_value(&mut self, value: u64) -> Result<(), RecordError> {
    let _writer = self.core.begin_writer_critical_section();
    let active = self.core.active();
    unsafe { (&mut *active).record_value(value) }
}
```

Safety comes from both parts:

- `&mut self` and the unique writer handle prevent concurrent mutable histogram
  access by another writer.
- The phaser prevents the sampler from exposing or recycling a histogram until
  any writer that could still hold its pointer has completed.

The active pointer must be loaded after entering the phaser writer critical
section. The writer guard must cover the entire histogram mutation.

### Beginning an interval sample

The sampler owns an inactive histogram that no writer can access:

1. Take the inactive `Box<T>` from the sampler.
2. Clear/reset it for reuse and set its interval start timestamp.
3. Acquire the phaser reader lock.
4. Convert the inactive box to a raw pointer and swap it into
   `active_histogram`.
5. Flip the phaser and wait for pre-swap writers to leave their critical
   sections.
6. Reconstitute the previous active pointer as the sampled `Box<T>`.
7. Set the sampled histogram's interval end timestamp.
8. Release the phaser reader lock.
9. Return an interval sample borrowing `&mut sampler`.

Writers that enter after the pointer swap use the new active histogram and do
not need to wait for the sampler. Writers that entered before the swap may
finish against either pointer ordering allowed by the protocol; the phase flip
waits conservatively until all old-phase operations have completed.

### Resampling

`SingleWriterIntervalSample::resample(self)` consumes the sample:

1. Reset its currently sampled histogram.
2. Use that histogram as the next inactive buffer in the same swap-and-flip
   operation.
3. Replace the sample's histogram with the newly stabilized previous active
   histogram.
4. Return `self`.

### Dropping a sample

Dropping a sample returns its `Box<T>` to the borrowed sampler's inactive slot.
It must not free the buffer. The mutable sampler borrow ensures the sampler
cannot begin another sample until the first sample is resampled or dropped.

The phaser reader guard no longer needs to live for the entire interval-sample
lifetime. It is local to each pointer swap and phase flip.

## Required safety invariants

Document these next to the unsafe pointer operations and in
`CONCURRENCY_INVARIANTS.md`:

1. Exactly one writer handle is constructed and recording requires `&mut`.
2. A writer enters the phaser before loading the active pointer.
3. A writer keeps its phaser guard until all mutation through that pointer is
   complete.
4. The sampler only resets a histogram it owns outside the active pointer.
5. The sampler swaps before flipping the phase.
6. The previous active pointer is not exposed, reset, or freed until the flip
   completes.
7. A sample's mutable borrow of its sampler is the exclusive sampling lease.
8. `RecorderCore` frees only its active histogram; the sampler/sample owns and
   frees the other buffer.
9. Active pointer publication must retain release/acquire ordering. Keep the
   existing `SeqCst` swap initially; weaken it only with a separate proof and
   model test.

## Auto-resize and double range behavior

The two buffers start with the same current internal model. Once the handles are
running, each buffer may auto-resize independently while active. Resetting a
buffer does not shrink its backing allocation or configured double ratio, so
capacity learned by each buffer is retained when it is recycled.

The sampler must not read mutable fields from the live active plain histogram.
The phaser only makes the previous active histogram stable after the swap and
flip; it does not provide a coherent pre-swap active-histogram snapshot. This
avoids reproducing Java's unsynchronized pre-swap copy of
`configuredHighestToLowestValueRatio`.

It is acceptable for the next active buffer to have less capacity than the
previous active buffer. With auto-resize enabled it may expand again; with
auto-resize disabled neither buffer can have expanded dynamically. This is a
performance characteristic, not a count-correctness issue.

If benchmarks demonstrate unacceptable alternating expansion cost, address it
as a separate enhancement by publishing construction-model changes from the
resize slow path. Do not add ratio/capacity checks or branches to the ordinary
recording fast path.

## Policy type rename

Rename the concrete generic types and give them default throwing policies:

| Current | Target |
| --- | --- |
| `DoubleHistogramWithPolicy<P>` | `DoubleHistogram<P = ThrowOnOverflow>` |
| `ConcurrentDoubleHistogramWithPolicy<P>` | `ConcurrentDoubleHistogram<P = ThrowOnOverflow>` |
| `DoubleRecorderWithPolicy<P>` | `DoubleRecorder<P = ThrowOnOverflow>` |
| `SingleWriterDoubleRecorderWithPolicy<P>` | `SingleWriterDoubleRecorder<P = ThrowOnOverflow>` |

Add the new generic sampler:

```rust
SingleWriterDoubleSampler<P = ThrowOnOverflow>
```

Retain the existing saturating aliases, retargeted to the renamed generic
types:

```rust
pub type SaturatingDoubleHistogram =
    DoubleHistogram<SaturateOnOverflow>;
pub type SaturatingConcurrentDoubleHistogram =
    ConcurrentDoubleHistogram<SaturateOnOverflow>;
pub type SaturatingDoubleRecorder =
    DoubleRecorder<SaturateOnOverflow>;
pub type SaturatingSingleWriterDoubleRecorder =
    SingleWriterDoubleRecorder<SaturateOnOverflow>;
pub type SaturatingSingleWriterDoubleSampler =
    SingleWriterDoubleSampler<SaturateOnOverflow>;
```

Builder types may keep their existing names, but should also use a default
throwing policy parameter where that improves direct type use:

- `DoubleHistogramBuilder<P = ThrowOnOverflow>`
- `ConcurrentDoubleHistogramBuilder<P = ThrowOnOverflow>`
- `DoubleRecorderBuilder<P = ThrowOnOverflow>`
- `SingleWriterDoubleRecorderBuilder<P = ThrowOnOverflow>`

Update imports, signatures, links, and exports in:

- `src/st/double_histogram.rs`
- `src/st/mod.rs`
- `src/concurrent/double_histogram.rs`
- `src/concurrent/recorder.rs`
- `src/concurrent/interval_sample.rs`
- `src/concurrent/mod.rs`
- `src/encoding.rs`
- `src/lib.rs`
- unit tests, integration tests, benchmarks, README examples, and rustdoc links.

This is intentionally a breaking rename. Do not leave public aliases with the
old `*WithPolicy` names unless explicitly requested.

## Implementation sequence

### Phase 1: Mechanical policy rename

1. Rename the four concrete generic types.
2. Add default `ThrowOnOverflow` generic parameters.
3. Remove the old default-policy aliases that now conflict with the concrete
   names.
4. Retarget saturating aliases.
5. Update all internal imports, generic signatures, exports, tests, benchmarks,
   README examples, and rustdoc links.
6. Run formatting, the complete test suite, and doc tests before beginning the
   synchronization refactor. Keeping this phase mechanical makes later failures
   easier to localize.

### Phase 2: Empty-buffer construction

1. Add internal empty-like construction for `Histogram`.
2. Add internal empty-like construction for `DoubleHistogram<P>`.
3. Verify that counts and tracking metadata are empty while layout, capacity,
   conversion metadata, configured ratio, significant digits, and auto-resize
   behavior are preserved as specified.
4. Add focused tests before integrating the helpers into recorders.

### Phase 3: Split handles

1. Add `SingleWriterSampler` and `SingleWriterDoubleSampler<P>`.
2. Change both single-writer builders to create an active and inactive buffer
   and return `(recorder, sampler)`.
3. Change both `from_histogram()` constructors to return the same pair.
4. Move the shared `RecorderCore<T>` behind `Arc` for these handles.
5. Make writer recording methods require `&mut self`.
6. Make sampler entry require `&mut self`.

### Phase 4: Phaser-only handoff

1. Move single-writer recording operations onto phaser writer critical
   sections.
2. Implement sampler-owned inactive buffer swapping and phase flipping.
3. Refactor single-writer interval samples to borrow the sampler and return the
   buffer on drop.
4. Remove `SingleWriterCore`, `SingleWriterRecordingGuard`,
   `SingleWriterSamplingGuard`, state constants, and `begin_sampling()`.
5. Remove cached inactive settings and fresh-histogram allocation from sampling.
6. Remove long-lived `PhaseFlipGuard` storage from the single-writer interval
   sample types.
7. Keep the multi-writer recorder implementations unchanged.

### Phase 5: Documentation and cleanup

1. Update README construction and threading examples.
2. Update `CONCURRENCY_INVARIANTS.md` with the unique-writer and sampler-owned
   inactive-buffer proof.
3. Document that builders allocate two buffers and return two handles.
4. Document that the writer handle is not cloneable and requires mutable access.
5. Document buffer-specific auto-resize capacity behavior.
6. Remove stale comments describing CAS sampling exclusion or sampler
   starvation.

## Test plan

### Construction and API

- Integer and double builders return writer/sampler pairs.
- Throwing and saturating double builders infer the intended policy.
- `from_histogram()` preserves the source as active and creates an empty
  model-compatible inactive histogram.
- Compile-fail rustdoc examples demonstrate that recording needs a mutable
  writer and that the writer cannot be cloned.

### Buffer lifecycle

- First sample contains all pre-boundary counts.
- Counts recorded after a swap appear only in the next sample.
- Repeated `resample()` loses and duplicates no counts.
- Dropping a sample returns its buffer; beginning a later sample does not
  allocate or panic.
- Dropping writer and sampler handles in either order frees both buffers exactly
  once.
- Interval start/end timestamps remain contiguous.
- Reset metadata and empty min/max sentinels remain correct.

### Concurrency

- Adapt the current single-writer stress tests to move the writer and sampler
  handles to separate threads.
- Repeated sampling during a hot writer loop preserves the aggregate count and
  value sum.
- Coordinated-omission correction remains within one interval.
- Add a loom model for the unique-writer phaser path covering writer entry,
  pointer load, swap, flip, sample exposure, and buffer reuse.
- Model and stress-test sample drop followed by a later new sample.

### Range and overflow policy

- Integer and double auto-resize work independently in both recycled buffers.
- A double buffer retains its expanded configured ratio after reset/reuse.
- Sampling does not copy or read range fields from the live active histogram.
- `auto_resize(false)` is preserved in both buffers.
- Throwing and saturating policies behave identically before and after swaps.
- Encoding sampled double histograms preserves the configured ratio and counts.

### Rename coverage

- No `WithPolicy` identifier remains in exported Rust APIs, rustdoc, README,
  tests, or benchmarks.
- Default generic policy use compiles without explicit type parameters.
- Saturating aliases resolve to the renamed concrete generic types.

## Performance validation

The new single-writer hot path replaces one successful CAS plus a release store
with phaser writer entry and exit operations. Measure rather than assume the
impact.

Run at least:

- `SingleWriterRecorder::record_value`
- `SingleWriterDoubleRecorder::record_value`
- counted recording
- coordinated-omission recording
- no-sampling throughput
- throughput with periodic sampling
- sampler latency under a continuously active writer

Compare against the current benchmark baselines recorded in the README. The
implementation should not be merged without explicitly reviewing the change in
single-writer record latency. Histogram recording implementations themselves
must show no regression attributable to this refactor.

### Recorded implementation results

On the README benchmark machine and command shape, the completed phaser-only
implementation measured:

| Benchmark | Result | Previous published result | Delta |
| --- | ---: | ---: | ---: |
| `SingleWriterRecorder::record_value` | 3.62 ns | 3.37 ns | +7.4% |
| `SingleWriterDoubleRecorder::record_value` | 5.72 ns | 5.39 ns | +6.1% |
| integer counted recording | 3.62 ns | n/a | n/a |
| integer coordinated-omission recording | 32.0 ns | n/a | n/a |
| integer recording with sampling every 1,024 calls | 5.76 ns | n/a | n/a |
| sampling under an active writer | 2.34 µs | n/a | n/a |
| double counted recording | 8.28 ns | n/a | n/a |
| double coordinated-omission recording | 94.7 ns | n/a | n/a |

The first two results use 30 samples, a one-second warm-up, and a three-second
measurement. The remaining new-path measurements use Criterion's `--quick`
mode. The implementation retains the requested phaser-only protocol; the
published hot-path deltas should be explicitly accepted before merge.

## Completion criteria

- The builder directly returns unique writer and sampler handles for both
  single-writer variants.
- Safe Rust cannot issue two simultaneous recording calls through those handles.
- Single-writer sampling uses only the phaser for writer/sampler handoff.
- No sampler operation changes a CAS state observed by the writer.
- No mutable live-histogram settings are read by the sampler.
- No inactive construction settings are cached in a recorder.
- Sampled buffers are recycled and freed exactly once.
- All policy-bearing concrete types use the renamed default-generic API.
- All unit, integration, loom, and doc tests pass.
- Formatting and lint checks pass.
- Benchmark deltas are recorded and reviewed.
