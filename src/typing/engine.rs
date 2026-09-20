use crate::{config::settings::Settings, input::windows, text::TextAnalysis};
use crossbeam_channel::{Receiver, Sender};
use rand::Rng;
use std::{
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub enum EngineCommand {
    Start(String, Settings),
    StartLast,
    Pause,
    Resume,
    TogglePause,
    Stop,
}

#[derive(Clone, Debug, Default)]
pub struct EngineStats {
    pub typed: usize,
    pub total: usize,
    pub mistakes: u32,
    pub corrections: u32,
    pub revisions: u32,
    pub started_at: Option<Instant>,
    pub paused: bool,
    pub finished: bool,
}

#[derive(Clone, Debug)]
pub enum EngineEvent {
    Stats(EngineStats),
    State(String),
    Error(String),
}

pub struct TypingEngine;

impl TypingEngine {
    pub fn spawn(events: Sender<EngineEvent>) -> Sender<EngineCommand> {
        let (commands, receiver) = crossbeam_channel::unbounded();
        thread::spawn(move || worker(receiver, events));
        commands
    }
}

fn worker(receiver: Receiver<EngineCommand>, events: Sender<EngineEvent>) {
    let mut active: Option<Run> = None;
    let mut last_start: Option<(String, Settings)> = None;
    loop {
        if active.is_none() {
            match receiver.recv() {
                Ok(EngineCommand::Start(text, settings)) => {
                    last_start = Some((text.clone(), settings.clone()));
                    active = start_run(&receiver, &events, text, settings);
                }
                Ok(EngineCommand::StartLast) => {
                    if let Some((text, settings)) = last_start.clone() {
                        active = start_run(&receiver, &events, text, settings);
                    }
                }
                Ok(_) => continue,
                Err(_) => break,
            }
        }
        let Some(run) = active.as_mut() else { continue };
        match receiver.try_recv() {
            Ok(EngineCommand::Pause) => run.paused = true,
            Ok(EngineCommand::Resume) => run.paused = false,
            Ok(EngineCommand::TogglePause) => run.paused = !run.paused,
            Ok(EngineCommand::Stop) => {
                let _ = events.send(EngineEvent::State("Stopped".into()));
                active = None;
                continue;
            }
            Ok(EngineCommand::Start(_, _)) => {}
            Ok(EngineCommand::StartLast) => {}
            Err(_) => {}
        }
        if run.paused {
            let _ = events.send(EngineEvent::Stats(run.stats()));
            thread::sleep(Duration::from_millis(50));
            continue;
        }
        if run.position >= run.text.len() {
            let mut stats = run.stats();
            stats.finished = true;
            let _ = events.send(EngineEvent::Stats(stats));
            let _ = events.send(EngineEvent::State("Finished".into()));
            active = None;
            continue;
        }
        let character = run.text[run.position..].chars().next().unwrap();
        let mut rng = rand::rng();
        if run.settings.thinking_enabled
            && (character == '\n'
                || run
                    .analysis
                    .tokens
                    .iter()
                    .any(|token| token.start == run.position && token.difficult))
            && rng.random_bool((run.settings.thinking_probability * 2.0).clamp(0.0, 1.0) as f64)
        {
            let pause =
                rng.random_range(run.settings.thinking_min_ms..=run.settings.thinking_max_ms);
            if !interruptible_sleep(&receiver, pause, run) {
                active = None;
                continue;
            }
        }
        if run.settings.mistakes_enabled
            && rng.random_bool(run.settings.mistake_rate.clamp(0.0, 1.0) as f64)
            && character.is_alphanumeric()
        {
            windows::send_char(neighbor_key(character));
            run.mistakes += 1;
            if !interruptible_sleep(&receiver, rng.random_range(90..=260), run) {
                active = None;
                continue;
            }
            windows::send_backspace();
            run.corrections += 1;
            if !interruptible_sleep(&receiver, rng.random_range(45..=140), run) {
                active = None;
                continue;
            }
        }
        windows::send_char(character);
        run.position += character.len_utf8();
        run.stats_typed += 1;
        if run.settings.revision_enabled
            && !run.revision_done
            && run.revisions < run.settings.max_revisions
            && run.started.elapsed().as_secs() >= run.settings.revision_min_seconds
            && run.started.elapsed().as_secs() <= run.settings.revision_max_seconds
        {
            let candidate = run
                .analysis
                .tokens
                .iter()
                .filter(|token| {
                    token.end <= run.position
                        && token.text.chars().count() >= 5
                        && !token.text.eq_ignore_ascii_case("there")
                })
                .max_by_key(|token| token.text.chars().count());
            if let Some(token) = candidate {
                if rng.random_bool(run.settings.revision_probability.clamp(0.0, 1.0) as f64) {
                    let replacement = match token.text.to_ascii_lowercase().as_str() {
                        "good" => "surprisingly good",
                        "nice" => "thoughtful",
                        "big" => "substantial",
                        _ => "better",
                    };
                    windows::send_revision_navigation(
                        run.text[..token.start].chars().count(),
                        token.text.chars().count(),
                        replacement,
                    );
                    run.revisions += 1;
                    run.revision_done = true;
                }
            }
        }
        let target_wpm = if run.settings.use_target_time {
            (run.analysis.characters as f32 / 5.0) / (run.settings.target_seconds / 60.0)
        } else {
            run.settings.wpm
        };
        let delay =
            crate::typing::timing::delay_for(character, &run.settings, &mut rng, target_wpm);
        if !interruptible_sleep(&receiver, delay, run) {
            active = None;
            continue;
        }
        let _ = events.send(EngineEvent::Stats(run.stats()));
    }
}

fn interruptible_sleep(
    receiver: &Receiver<EngineCommand>,
    milliseconds: u64,
    run: &mut Run,
) -> bool {
    let deadline = Instant::now() + Duration::from_millis(milliseconds);
    while Instant::now() < deadline {
        match receiver.recv_timeout(Duration::from_millis(20)) {
            Ok(EngineCommand::Pause) => run.paused = true,
            Ok(EngineCommand::Resume) => run.paused = false,
            Ok(EngineCommand::TogglePause) => run.paused = !run.paused,
            Ok(EngineCommand::Stop) => return false,
            Ok(EngineCommand::Start(_, _) | EngineCommand::StartLast) => {}
            Err(_) => {}
        }
        if run.paused {
            return true;
        }
    }
    true
}

struct Run {
    text: String,
    position: usize,
    settings: Settings,
    analysis: TextAnalysis,
    stats_typed: usize,
    mistakes: u32,
    corrections: u32,
    revisions: u32,
    revision_done: bool,
    paused: bool,
    started: Instant,
}
impl Run {
    fn new(text: String, settings: Settings) -> Self {
        Self {
            analysis: TextAnalysis::parse(&text),
            text,
            position: 0,
            settings,
            stats_typed: 0,
            mistakes: 0,
            corrections: 0,
            revisions: 0,
            revision_done: false,
            paused: false,
            started: Instant::now(),
        }
    }
    fn stats(&self) -> EngineStats {
        EngineStats {
            typed: self.stats_typed,
            total: self.analysis.characters,
            mistakes: self.mistakes,
            corrections: self.corrections,
            revisions: self.revisions,
            paused: self.paused,
            started_at: Some(self.started),
            ..Default::default()
        }
    }
}

fn start_run(
    receiver: &Receiver<EngineCommand>,
    events: &Sender<EngineEvent>,
    text: String,
    settings: Settings,
) -> Option<Run> {
    for remaining in (1..=settings.countdown_seconds).rev() {
        let _ = events.send(EngineEvent::State(format!("Starting in {}", remaining)));
        match receiver.recv_timeout(Duration::from_secs(1)) {
            Ok(EngineCommand::Stop) => return None,
            Ok(_) | Err(_) => {}
        }
    }
    let _ = events.send(EngineEvent::State("Typing".into()));
    Some(Run::new(text, settings))
}

fn neighbor_key(character: char) -> char {
    match character.to_ascii_lowercase() {
        'q' => 'w',
        'w' => 'e',
        'e' => 'r',
        'r' => 't',
        't' => 'y',
        'y' => 'u',
        'u' => 'i',
        'i' => 'o',
        'o' => 'p',
        'a' => 's',
        's' => 'd',
        'd' => 'f',
        'f' => 'g',
        'g' => 'h',
        'h' => 'j',
        'j' => 'k',
        'k' => 'l',
        'z' => 'x',
        'x' => 'c',
        'c' => 'v',
        'v' => 'b',
        'b' => 'n',
        'n' => 'm',
        'm' => 'n',
        _ => character,
    }
}
