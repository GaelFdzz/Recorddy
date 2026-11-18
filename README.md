# Recorddy — Open-Source Screen Recorder for Windows

**Recorddy** is a Windows screen recorder built with Rust, designed for instant replay clips and long-form screen recording.

Capture gameplay, desktop activity and videos using GPU-accelerated H.264 encoding, without requiring a traditional installer.

## Features

- **Instant Replay:** Save the last seconds or minutes of gameplay.
- **Continuous Recording:** Record full sessions and stop whenever you want.
- **GPU-Accelerated Recording:** H.264 encoding using Windows Media Foundation and compatible hardware.
- **Multi-Monitor Support:** Automatically follow the active screen or select a monitor manually.
- **Audio Capture:** Capture desktop audio and supported microphone input.
- **Recording Modes:** Choose between instant replay and continuous recording.
- **Built-in Video Editor:** Review, trim and export recordings.
- **Windows Native:** Built in Rust with native Windows APIs.

## Características en español

Recorddy es un grabador de pantalla de código abierto para Windows, desarrollado en Rust. Permite guardar clips instantáneos o grabar sesiones completas, utilizando codificación H.264 acelerada por hardware cuando está disponible.

## Build from source

Requires Rust, Cargo, and the Microsoft C++ build tools on Windows.

```powershell
cargo build --release
```

The executable is generated at `target/release/recorddy.exe`.

## License and attribution

Recorddy is a modified fork of [Clippy by santiremedi](https://github.com/santiremedi/clippy).

The project is distributed under **GPL-3.0-or-later**, retaining the original copyright notices and license obligations.

Recorddy is an independent modified version and is not an official Clippy release.
