# Dripwriter

Dripwriter is a free, local Windows desktop app that types text into the currently focused input using native Windows `SendInput` events. It does not use Google Docs, browser automation, a cloud service, OpenGL, or third-party runtime dependencies.

The executable is a native Windows PE file built from Rust's standard library and direct Win32 FFI. Windows system DLLs such as `user32.dll` are part of Windows itself; no DLL, Python runtime, Node.js runtime, VC++ redistributable, or graphics runtime is bundled or required.

## Architecture

The native Win32 window owns the text editor and schedule controls. Starting a run creates a standard-library `mpsc` channel and a worker thread. That thread generates a humanized action plan and calls Win32 `SendInput` directly. Pause, resume, and stop messages are drained between actions and during 100 ms sleep slices, so the UI stays responsive and cancellation does not wait for a long punctuation or thinking pause to finish.

Progress and control state are held in `Arc<Atomic*>` values. This avoids sharing the input simulator across threads and keeps the GUI's polling cheap.

## Build on Windows

Install the stable Rust toolchain, then run:

```powershell
cargo build --release
```

The executable is `target\release\dripwriter.exe`. The release profile enables link-time optimization, strips symbols, and aborts on panic. The release workflow statically links the MSVC runtime, so the published PE does not require the Visual C++ redistributable. It does not require Python, Node.js, OpenGL, or a separate application runtime. Launch it, paste text, set the duration, click **Start Dripping**, and focus the destination text field.

To test the UI state machine without sending any keystrokes, run:

```powershell
.\dripwriter.exe --self-test
```

This opens a result dialog after checking invalid input, start, pause, resume, progress completion, stop, and duration clamping. The same checks also run with `cargo test` during development.

The app writes its last text and duration to `dripwriter.json` beside the executable. Do not place credentials or sensitive text in that file.

## GitHub release

Push a version tag to build and attach the executable automatically:

```powershell
git tag v0.1.0
git push origin v0.1.0
```

The workflow in `.github/workflows/release.yml` builds the `dripwriter-windows-x86_64.exe` asset for 64-bit Windows.

## Behavior notes

Typing speed is derived from word count and the selected schedule. Each character receives a normally distributed delay, with extra punctuation and occasional thinking pauses. With a 2% chance on supported QWERTY letters, the worker types one physically adjacent key, waits 300-600 ms, backspaces, and types the intended key. Runs can be paused or stopped from the app at any time.

The application needs permission to send keyboard input to the active desktop. Always verify the destination field is focused before starting a run.# autowrite