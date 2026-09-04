# Concurrency Invariants

This crate follows the Java HdrHistogram writer-reader phaser model for the
concurrent histogram and recorder paths. The details below are the invariants
that future changes must preserve.

## Recorder Pointer Publication

- `Recorder` swaps the active histogram pointer while holding the reader lock.
- The pointer swap is `SeqCst`, which provides release publication of the
  cleared or freshly allocated inactive histogram.
- Writer paths must load the active histogram pointer with at least `Acquire`
  before recording. This makes the clear/reset work that happened before the
  swap visible before the writer updates counts.
- The phaser flip after the swap waits for writers that started before the swap
  to finish before the sampled histogram is exposed to readers.
- Writer exit publishes count updates with a release phase-end increment; the
  flip-side catch-up load must acquire that increment before sampled counts are
  exposed.
- An interval sample may recycle its sampled histogram only after clearing it
  and republishing it through the recorder swap.

## Single-Writer Recorder Ownership

- A single-writer builder constructs exactly one writer handle and one sampler
  handle, with both histogram buffers allocated before either handle is
  returned.
- Recording requires `&mut` access to the unique, non-cloneable writer handle.
  This ownership contract replaces the old recording/sampling CAS state
  machine; unsafe duplication or concurrent use of the writer is unsupported.
- The writer enters the phaser before loading the active histogram pointer and
  retains its writer guard until the complete logical mutation, including
  coordinated-omission correction, has finished.
- The sampler owns the inactive `Box`. An interval sample mutably borrows the
  sampler and temporarily owns that box; dropping the sample returns it to the
  sampler rather than freeing it.
- The sampler resets only its inactive buffer, acquires the reader lock, swaps
  that buffer into the active pointer, and then flips the phase. It must not
  expose, reset, or free the previous active pointer until the flip has waited
  for all old-phase writers.
- The reader lock is local to the swap and flip. It is not held for the lifetime
  of a single-writer interval sample.
- The shared recorder core owns and eventually frees only the pointer currently
  in the active slot. The sampler or its interval sample owns the other buffer,
  so dropping the two handles in either order frees each buffer once.
- Single-writer double buffers retain auto-resized capacity independently.
  Sampling does not inspect or copy mutable settings from the live active plain
  histogram.

## Resizable Counts Array Publication

- `ResizableConcurrentHistogram` keeps active and inactive count arrays so range shifts
  and resizes can prepare the inactive array before publication.
- Any change to count-array interpretation, including
  `normalizing_index_offset` and the double conversion ratio, must be written to
  the inactive array before it is published as active.
- Publishing the prepared inactive array uses a `SeqCst` store to
  `active_counts`.
- Recording paths load `active_counts` with `Acquire` before reading offset or
  conversion-ratio metadata from the array. Those metadata reads may remain
  relaxed because they are ordered after the acquire pointer load.
- Both arrays must be updated across two flips for metadata-only changes so
  either array can later become active with the same interpretation.

## Concurrent Double Histogram Range Shifts

- Double histograms store counts in an integer histogram and publish the
  double-to-integer conversion ratio with the active count array.
- Slot selection must read the conversion ratio from the same active array that
  receives the count update.
- Range-shift paths update the active range limits and then shift or republish
  the backing arrays while holding the range lock.
- Empty and zero-only histograms still need to publish offset and conversion
  ratio changes, even when there are no non-zero buckets to move.
- Saturating double histograms may clamp slot selection, but normal recording
  should still avoid saturating the count addition path for performance.

## Testing Expectations

- Recorder stress tests must combine active writers with repeated interval
  sampling and verify no counts are lost.
- Double histogram tests must cover empty range shifts, zero-only range shifts,
  and concurrent range-shift publication under active recording.
- If future changes weaken the simple acquire/release reasoning above, add a
  small model test for the affected publication path before changing the
  implementation.
