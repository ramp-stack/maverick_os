# Maverick OS

Maverick OS provides common hardware interfaces for applications.

## Building

Video validation and playback use FFmpeg libraries built automatically by Cargo through
`ramp-ffmpeg`. FFmpeg 9.0.2 source ships in the transitive binding crate and is compiled for the
target. The libraries are linked statically into the application, so building and running
Maverick does not require an installed FFmpeg package or runtime. Building FFmpeg requires
neither Git nor a separate source download.

macOS and iOS builds use pregenerated FFmpeg bindings and require Rust, Xcode's native compiler
and target SDK, and Make. Intel targets also require NASM. These builds do not require libclang
or pkg-config. Subsequent builds reuse Cargo's build cache.

Run the hardware demo with:

```sh
cargo run --features demo
```
