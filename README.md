# HdrHistogram-Rust

Port of HdrHistogram to Rust.

This port aims to be feature complete but is still missing
an implementation of the log reader and the double histogram
versions. Unfortunately I have had very little time in the last
few months and it might still be a few weeks before these missing
features are implemented.

The concurrent histograms and recorder are somewhat feature complete
and will probably work for your needs.
A log writer is implemented but the corresponding reader is 
not done yet. I'm not entirely sure I like the log format anyway 
and I assume you'd be building your own for your particular needs.

So far I've only made sure that the single threaded histogram is
very reliably fast, but since the key to achieving this was mostly
to make sure that the right things are `#[inline(never)]`,  
the concurrent version should be pretty reasonable as well.
A value recording takes about (3.1 +/- 0.4)ns to complete.

Note that this uses a bunch of unstable features. If you need a 
version that uses only stable features or don't need a recorder, 
you should head over to
https://github.com/HdrHistogram/HdrHistogram_rust
and use that instead.

## Quick overview

### Single threaded version

The single threaded version of the histogram can be found in the st
submodule. I recommend reading the tests in `tests/histogram.rs` as
further documentation.

### Concurrent version

I recommend not using the concurrent histograms directly but using
a recorder instead.

Currently, there are only two constructors

```rust
use hdrhistogram_rust::concurrent::recorder;
recorder::resizable_with_low_high_sigvdig
recorder::static_with_low_high_sigvdig
```

which produce recorders that use an underlying
`concurrent::resizable_histogram`, which corresponds to the 
`ConcurrentHistogram` of the Java version and a 
`concurrent::static_histogram`, which corresponds to the `AtomicHistogram`
of the Java version.

The current counts can be obtained via the `locking_sample()` method.
It returns a guard object that allows you to safely recycle the histogram
using the `resample()` method and to safely obtain a snapshot via `snapshot()`.
Note that this does actually lock the reader side to guarantee exclusive
access. The writers can however still proceed without slowdown.

I assumed that this would be the most common use case anyway,
since the recorders will probably mostly be read by a single stats
consumer that logs the snapshots on a periodic basis.

An unsafe method allowing concurrent sampling will follow.

### Iteration and serialization

Both the single threaded histogram as well as any 
`Snapshot<T>` of a concurrent histogram are iterable and serializable.
I recommend reading the tests in `tests/iteration.rs` and 
`tests/serialization.rs` for further details.
