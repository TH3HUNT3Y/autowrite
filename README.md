# Dripwriter

Dripwriter is a free, local Windows desktop app that types text into the currently focused input using OS-level keyboard events. It does not use Google Docs, browser automation, a cloud service, or a runtime dependency.

## Architecture

The `eframe` UI owns the text editor and schedule controls. Starting a run creates a standard-library `mpsc` channel and a worker thread. That thread creates its own single-thread Tokio runtime, generates a humanized action plan, owns the `enigo::Enigo` instance, and executes the plan. Pause, resume, and stop messages are drained between actions and during 100 ms sleep slices, so the UI stays responsive and cancellation does not wait for a long punctuation or thinking pause to finish.

Progress and control state are held in `Arc<Atomic*>` values. This avoids sharing the input simulator across threads and keeps the GUI's polling cheap.

## Build on Windows

Install the stable Rust toolchain, then run:

```powershell
cargo build --release
```

The executable is `target\release\dripwriter.exe`. The release profile enables link-time optimization, strips symbols, and aborts on panic. A normal Windows build uses the Microsoft toolchain and does not require Python, Node.js, or a separate application runtime. Launch it, paste text, set the duration, click **Start Dripping**, and focus the destination text field.

The app writes its last text and duration to `dripwriter.json` beside the executable. Do not place credentials or sensitive text in that file.

## GitHub release

Push a version tag to build and attach the executable automatically:

```powershell
git tag v0.1.0
git push origin v0.1.0
```

The workflow in `.github/workflows/release.yml` builds two release assets:

- `dripwriter-windows-x86_64.exe` for 64-bit Windows.
- `dripwriter-linux-x86_64` for Linux desktop environments, including the Linux development environment on supported Chromebooks.

On a Chromebook, enable **Linux development environment** in ChromeOS Settings, download the Linux asset, mark it executable in the Files app, and open it from the Linux files area. Crosh is not required. ChromeOS may still block OS-level input from a Linux app into Chrome browser tabs; the Linux build is intended for Linux GUI applications and text fields inside the Linux environment. A browser extension would be required to type into Chrome tabs, and would not provide universal typing into every ChromeOS app.

## Behavior notes

Typing speed is derived from word count and the selected schedule. Each character receives a normally distributed delay, with extra punctuation and occasional thinking pauses. With a 2% chance on supported QWERTY letters, the worker types one physically adjacent key, waits 300-600 ms, backspaces, and types the intended key. Runs can be paused or stopped from the app at any time.

The application needs permission to send keyboard input to the active desktop. Always verify the destination field is focused before starting a run.# autowrite