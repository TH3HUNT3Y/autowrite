# Auto Write

Auto Write is a Windows desktop typing simulator built in Rust. It sends Unicode keyboard input with `SendInput`, so the focused application does not need an integration or clipboard support.

## Development

```text
cargo check
cargo test
```

The container can validate the Windows implementation directly:

```text
cargo check --target x86_64-pc-windows-gnu
```

## Windows release build

Install the `x86_64-pc-windows-msvc` target on a Windows Rust installation, then run:

```text
cargo build --release --target x86_64-pc-windows-msvc
```

The executable is written to `target/x86_64-pc-windows-msvc/release/auto_write.exe`.

## Use

1. Enter or paste text in the Writer tab.
2. Choose WPM or target-time mode and tune the humanization settings.
3. Press Start, then switch to the destination application during the countdown.
4. F6 starts the last prepared run, F7 pauses/resumes, and F8 is the emergency stop.

Settings are saved in the platform configuration directory as `settings.json`.