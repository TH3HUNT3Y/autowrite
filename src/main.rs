use std::{
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    thread,
    time::Duration,
};

use eframe::egui;
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use rand::{thread_rng, Rng};
use rand_distr::{Distribution, Normal};
use serde::{Deserialize, Serialize};

const MINUTES_MIN: u64 = 10;
const MINUTES_MAX: u64 = 7 * 24 * 60;
const DEFAULT_TEXT: &str = "Paste text here, choose how long it should take, then place your cursor in the destination field.";

#[derive(Clone, Debug)]
enum TypingAction {
    KeyPress(char),
    Backspace,
    Pause(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WorkerMessage {
    Pause,
    Resume,
    Stop,
}

#[derive(Clone)]
struct WorkerState {
    paused: Arc<AtomicBool>,
    canceled: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    completed: Arc<AtomicUsize>,
    total: Arc<AtomicUsize>,
}

impl WorkerState {
    fn new() -> Self {
        Self {
            paused: Arc::new(AtomicBool::new(false)),
            canceled: Arc::new(AtomicBool::new(false)),
            failed: Arc::new(AtomicBool::new(false)),
            completed: Arc::new(AtomicUsize::new(0)),
            total: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn progress(&self) -> f32 {
        let total = self.total.load(Ordering::Relaxed);
        if total == 0 {
            0.0
        } else {
            self.completed.load(Ordering::Relaxed) as f32 / total as f32
        }
    }
}

fn adjacent_key(character: char) -> Option<char> {
    let lower = character.to_ascii_lowercase();
    let neighbor = match lower {
        'q' => 'w',
        'w' => 'e',
        'e' => 'r',
        'r' => 't',
        't' => 'y',
        'y' => 'u',
        'u' => 'i',
        'i' => 'o',
        'o' => 'p',
        'p' => 'o',
        'a' => 's',
        's' => 'd',
        'd' => 'f',
        'f' => 'g',
        'g' => 'h',
        'h' => 'j',
        'j' => 'k',
        'k' => 'l',
        'l' => 'k',
        'z' => 'x',
        'x' => 'c',
        'c' => 'v',
        'v' => 'b',
        'b' => 'n',
        'n' => 'm',
        'm' => 'n',
        _ => return None,
    };

    Some(if character.is_ascii_uppercase() {
        neighbor.to_ascii_uppercase()
    } else {
        neighbor
    })
}

fn humanize(text: &str, target_wpm: f64, typo_rate: f64) -> Vec<TypingAction> {
    let mut rng = thread_rng();
    let mean_ms = (60_000.0 / (target_wpm.max(1.0) * 5.0)).max(8.0);
    let normal = Normal::new(mean_ms, mean_ms * 0.22).expect("positive typing distribution");
    let mut actions = Vec::with_capacity(text.chars().count() * 2);
    let mut sentence_count = 0;

    for character in text.chars() {
        let mut delay = normal.sample(&mut rng).max(8.0) as u64;
        if character == ',' {
            delay += 150;
        } else if matches!(character, '.' | '!' | '?' | ';' | ':') {
            delay += 300;
            sentence_count += 1;
            if sentence_count >= 2 && rng.gen_bool(0.18) {
                delay += rng.gen_range(1_000..=3_000);
                sentence_count = 0;
            }
        }
        actions.push(TypingAction::Pause(delay));

        if rng.gen_bool(typo_rate.clamp(0.0, 1.0)) {
            if let Some(wrong) = adjacent_key(character) {
                actions.push(TypingAction::KeyPress(wrong));
                actions.push(TypingAction::Pause(rng.gen_range(300..=600)));
                actions.push(TypingAction::Backspace);
            }
        }
        actions.push(TypingAction::KeyPress(character));
    }

    actions
}

fn scale_to_duration(actions: &mut [TypingAction], duration: Duration) {
    let current_ms: u128 = actions
        .iter()
        .map(|action| match action {
            TypingAction::Pause(milliseconds) => *milliseconds as u128,
            _ => 0,
        })
        .sum();
    if current_ms == 0 {
        return;
    }

    let target_ms = duration.as_millis();
    let scale = target_ms as f64 / current_ms as f64;
    for action in actions {
        if let TypingAction::Pause(milliseconds) = action {
            *milliseconds = ((*milliseconds as f64 * scale).round() as u64).max(1);
        }
    }
}

fn estimated_wpm(text: &str, duration_minutes: u64) -> f64 {
    let words = text.split_whitespace().count() as f64;
    if words == 0.0 {
        0.0
    } else {
        words / duration_minutes.max(1) as f64
    }
}

async fn sleep_with_controls(
    milliseconds: u64,
    state: &WorkerState,
    receiver: &Receiver<WorkerMessage>,
) -> bool {
    let mut remaining = milliseconds;
    while remaining > 0 {
        drain_controls(state, receiver);
        if state.canceled.load(Ordering::Relaxed) {
            return false;
        }
        while state.paused.load(Ordering::Relaxed) {
            drain_controls(state, receiver);
            if state.canceled.load(Ordering::Relaxed) {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let slice = remaining.min(100);
        tokio::time::sleep(Duration::from_millis(slice)).await;
        remaining -= slice;
    }
    true
}

fn drain_controls(state: &WorkerState, receiver: &Receiver<WorkerMessage>) {
    while let Ok(message) = receiver.try_recv() {
        match message {
            WorkerMessage::Pause => state.paused.store(true, Ordering::Relaxed),
            WorkerMessage::Resume => state.paused.store(false, Ordering::Relaxed),
            WorkerMessage::Stop => state.canceled.store(true, Ordering::Relaxed),
        }
    }
}

async fn execute_actions(
    actions: Vec<TypingAction>,
    state: WorkerState,
    receiver: Receiver<WorkerMessage>,
) {
    let mut enigo = match Enigo::new(&Settings::default()) {
        Ok(enigo) => enigo,
        Err(_) => {
            state.failed.store(true, Ordering::Relaxed);
            return;
        }
    };

    state.total.store(actions.len(), Ordering::Relaxed);
    for action in actions {
        drain_controls(&state, &receiver);
        if state.canceled.load(Ordering::Relaxed) {
            break;
        }
        match action {
            TypingAction::Pause(milliseconds) => {
                if !sleep_with_controls(milliseconds, &state, &receiver).await {
                    break;
                }
            }
            TypingAction::KeyPress(character) => {
                let _ = enigo.key(Key::Unicode(character), Direction::Click);
            }
            TypingAction::Backspace => {
                let _ = enigo.key(Key::Backspace, Direction::Click);
            }
        }
        state.completed.fetch_add(1, Ordering::Relaxed);
    }
    state.paused.store(false, Ordering::Relaxed);
}

fn start_worker(text: String, duration: Duration) -> (Sender<WorkerMessage>, WorkerState) {
    let state = WorkerState::new();
    let worker_state = state.clone();
    let (sender, receiver) = mpsc::channel();
    let target_wpm = estimated_wpm(&text, duration.as_secs() / 60);
    thread::spawn(move || {
        let mut actions = humanize(&text, target_wpm.max(1.0), 0.02);
        scale_to_duration(&mut actions, duration);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("worker runtime should build");
        runtime.block_on(execute_actions(actions, worker_state, receiver));
    });
    (sender, state)
}

#[derive(Serialize, Deserialize)]
struct Preferences {
    duration_minutes: u64,
    text: String,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            duration_minutes: 30,
            text: DEFAULT_TEXT.to_owned(),
        }
    }
}

struct DripwriterApp {
    text: String,
    duration_minutes: u64,
    worker: Option<(Sender<WorkerMessage>, WorkerState)>,
    last_error: Option<String>,
}

impl DripwriterApp {
    fn load() -> Self {
        let preferences = std::fs::read_to_string("dripwriter.json")
            .ok()
            .and_then(|value| serde_json::from_str::<Preferences>(&value).ok())
            .unwrap_or_default();
        Self {
            text: preferences.text,
            duration_minutes: preferences.duration_minutes.clamp(MINUTES_MIN, MINUTES_MAX),
            worker: None,
            last_error: None,
        }
    }

    fn save_preferences(&self) {
        let preferences = Preferences {
            duration_minutes: self.duration_minutes,
            text: self.text.clone(),
        };
        if let Ok(serialized) = serde_json::to_string_pretty(&preferences) {
            let _ = std::fs::write("dripwriter.json", serialized);
        }
    }

    fn is_running(&self) -> bool {
        self.worker
            .as_ref()
            .map(|(_, state)| {
                state.progress() < 1.0
                    && !state.canceled.load(Ordering::Relaxed)
                    && !state.failed.load(Ordering::Relaxed)
            })
            .unwrap_or(false)
    }
}

impl eframe::App for DripwriterApp {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        context.request_repaint_after(Duration::from_millis(100));
        let progress = self
            .worker
            .as_ref()
            .map(|(_, state)| state.progress())
            .unwrap_or(0.0);
        let paused = self
            .worker
            .as_ref()
            .map(|(_, state)| state.paused.load(Ordering::Relaxed))
            .unwrap_or(false);

        egui::CentralPanel::default().show(context, |ui| {
            ui.add_space(24.0);
            ui.heading("Dripwriter");
            ui.label("A quiet, local text dripper for any active text field.");
            ui.add_space(18.0);

            ui.label("Text to type");
            ui.add(
                egui::TextEdit::multiline(&mut self.text)
                    .desired_rows(13)
                    .hint_text("Paste the text you want to type...")
                    .lock_focus(true),
            );
            ui.add_space(14.0);

            ui.horizontal(|ui| {
                ui.label("Duration");
                ui.add(
                    egui::Slider::new(&mut self.duration_minutes, MINUTES_MIN..=MINUTES_MAX)
                        .text("minutes"),
                );
                ui.label(format_duration(self.duration_minutes));
            });
            ui.label(format!(
                "Estimated pace: {:.1} WPM  |  {} words",
                estimated_wpm(&self.text, self.duration_minutes),
                self.text.split_whitespace().count()
            ));

            ui.add_space(18.0);
            ui.horizontal(|ui| {
                if !self.is_running() {
                    if ui.button("Start Dripping").clicked() && !self.text.trim().is_empty() {
                        self.save_preferences();
                        self.last_error = None;
                        self.worker = Some(start_worker(
                            self.text.clone(),
                            Duration::from_secs(self.duration_minutes * 60),
                        ));
                    }
                } else if paused {
                    if ui.button("Resume").clicked() {
                        if let Some((sender, _)) = &self.worker {
                            let _ = sender.send(WorkerMessage::Resume);
                        }
                    }
                } else if ui.button("Pause").clicked() {
                    if let Some((sender, _)) = &self.worker {
                        let _ = sender.send(WorkerMessage::Pause);
                    }
                }

                if self.worker.is_some() && ui.button("Stop").clicked() {
                    if let Some((sender, state)) = &self.worker {
                        state.canceled.store(true, Ordering::Relaxed);
                        let _ = sender.send(WorkerMessage::Stop);
                    }
                }
            });

            if self.worker.is_some() {
                ui.add_space(12.0);
                ui.add(egui::ProgressBar::new(progress).show_percentage());
                let failed = self
                    .worker
                    .as_ref()
                    .map(|(_, state)| state.failed.load(Ordering::Relaxed))
                    .unwrap_or(false);
                ui.label(if failed {
                    "Could not initialize keyboard input. Check desktop permissions."
                } else if paused {
                    "Paused"
                } else {
                    "Running. Focus the destination field now."
                });
            }
            if let Some(error) = &self.last_error {
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            }
        });
    }
}

fn format_duration(minutes: u64) -> String {
    if minutes >= 24 * 60 {
        format!("{}d {}h", minutes / (24 * 60), (minutes / 60) % 24)
    } else if minutes >= 60 {
        format!("{}h {}m", minutes / 60, minutes % 60)
    } else {
        format!("{}m", minutes)
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([720.0, 620.0])
            .with_min_inner_size([520.0, 460.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Dripwriter",
        options,
        Box::new(|creation_context| {
            let mut visuals = egui::Visuals::dark();
            visuals.window_rounding = egui::Rounding::same(8.0);
            creation_context.egui_ctx.set_visuals(visuals);
            Ok(Box::new(DripwriterApp::load()))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjacent_key_preserves_case() {
        assert_eq!(adjacent_key('a'), Some('s'));
        assert_eq!(adjacent_key('A'), Some('S'));
        assert_eq!(adjacent_key(' '), None);
    }

    #[test]
    fn duration_scaling_reaches_requested_pause_budget() {
        let mut actions = vec![
            TypingAction::Pause(100),
            TypingAction::KeyPress('a'),
            TypingAction::Pause(200),
        ];
        scale_to_duration(&mut actions, Duration::from_millis(6_000));
        let total: u64 = actions
            .iter()
            .map(|action| match action {
                TypingAction::Pause(milliseconds) => *milliseconds,
                _ => 0,
            })
            .sum();
        assert_eq!(total, 6_000);
    }
}
