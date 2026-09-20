#[cfg(windows)]
pub mod windows;

#[cfg(not(windows))]
pub mod windows {
    use crate::typing::EngineCommand;
    use crossbeam_channel::Sender;
    pub fn send_char(_character: char) {}
    pub fn send_backspace() {}
    pub fn send_revision_navigation(
        _character_count: usize,
        _word_length: usize,
        _replacement: &str,
    ) {
    }
    pub fn spawn_hotkeys(_commands: Sender<EngineCommand>) {}
}
