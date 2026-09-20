use crate::{
    config::settings::Settings,
    text::TextAnalysis,
    typing::{EngineCommand, EngineEvent, EngineStats, TypingEngine},
};
use eframe::egui::{self, Color32, RichText, Stroke, Vec2};
use std::time::{Duration, Instant};

const BG: Color32 = Color32::from_rgb(23, 25, 27);
const PANEL: Color32 = Color32::from_rgb(31, 34, 37);
const INK: Color32 = Color32::from_rgb(232, 235, 231);
const MUTED: Color32 = Color32::from_rgb(157, 166, 164);
const ORANGE: Color32 = Color32::from_rgb(232, 126, 52);
const TEAL: Color32 = Color32::from_rgb(92, 186, 166);

pub struct AutoWriteApp {
    text: String,
    settings: Settings,
    analysis: TextAnalysis,
    commands: crossbeam_channel::Sender<EngineCommand>,
    events: crossbeam_channel::Receiver<EngineEvent>,
    stats: EngineStats,
    status: String,
    tab: Tab,
    last_save: Instant,
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Writer,
    Timing,
    Humanization,
    Revisions,
    Hotkeys,
}

impl AutoWriteApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (events_sender, events) = crossbeam_channel::unbounded();
        let commands = TypingEngine::spawn(events_sender);
        crate::input::windows::spawn_hotkeys(commands.clone());
        let settings = Settings::load();
        configure_style(&cc.egui_ctx);
        Self {
            text: String::new(),
            analysis: TextAnalysis::default(),
            settings,
            commands,
            events,
            stats: EngineStats::default(),
            status: "Ready".into(),
            tab: Tab::Writer,
            last_save: Instant::now(),
        }
    }

    fn receive_events(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                EngineEvent::Stats(stats) => self.stats = stats,
                EngineEvent::State(status) => self.status = status,
                EngineEvent::Error(error) => self.status = error,
            }
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("AUTO WRITE").strong().size(18.0).color(INK));
            ui.label(RichText::new("/ focused-window typing studio").color(MUTED));
            ui.add_space(20.0);
            for (tab, name) in [
                (Tab::Writer, "Writer"),
                (Tab::Timing, "Timing"),
                (Tab::Humanization, "Humanization"),
                (Tab::Revisions, "Revisions"),
                (Tab::Hotkeys, "Hotkeys"),
            ] {
                let selected = self.tab == tab;
                if ui
                    .selectable_label(
                        selected,
                        RichText::new(name).color(if selected { INK } else { MUTED }),
                    )
                    .clicked()
                {
                    self.tab = tab;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(&self.status).color(if self.status == "Ready" {
                        MUTED
                    } else {
                        TEAL
                    }),
                );
                ui.label(RichText::new("●").color(if self.stats.finished { TEAL } else { ORANGE }));
            });
        });
        ui.add_space(10.0);
        ui.separator();
    }

    fn writer(&mut self, ui: &mut egui::Ui) {
        ui.columns(2, |columns| {
            columns[0].vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("SOURCE TEXT").strong().color(MUTED));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!(
                                "{} chars  /  {} words",
                                self.analysis.characters, self.analysis.words
                            ))
                            .color(MUTED),
                        );
                    });
                });
                let response = ui.add_sized(
                    ui.available_size() - Vec2::new(0.0, 30.0),
                    egui::TextEdit::multiline(&mut self.text)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .hint_text("Paste or write the text to type into the focused window..."),
                );
                if response.changed() {
                    self.analysis = TextAnalysis::parse(&self.text);
                }
            });
            columns[1].vertical(|ui| {
                ui.label(RichText::new("SESSION READOUT").strong().color(MUTED));
                ui.add_space(8.0);
                stat_pair(ui, "Progress", format!("{:.0}%", progress(&self.stats)));
                stat_pair(ui, "Current WPM", format_wpm(&self.stats));
                stat_pair(
                    ui,
                    "Characters",
                    format!("{} / {}", self.stats.typed, self.stats.total),
                );
                stat_pair(ui, "Mistakes", self.stats.mistakes.to_string());
                stat_pair(ui, "Corrections", self.stats.corrections.to_string());
                stat_pair(ui, "Revisions", self.stats.revisions.to_string());
                ui.add_space(14.0);
                ui.add(
                    egui::ProgressBar::new(progress(&self.stats) / 100.0)
                        .desired_height(8.0)
                        .fill(ORANGE),
                );
                ui.add_space(16.0);
                ui.label(RichText::new("TIMING MODE").strong().color(MUTED));
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.settings.use_target_time, false, "WPM");
                    ui.selectable_value(&mut self.settings.use_target_time, true, "Target time");
                });
                if self.settings.use_target_time {
                    ui.add(
                        egui::Slider::new(&mut self.settings.target_seconds, 10.0..=900.0)
                            .text("seconds"),
                    );
                } else {
                    ui.add(egui::Slider::new(&mut self.settings.wpm, 10.0..=180.0).text("WPM"));
                }
                ui.label(
                    RichText::new(format!(
                        "Estimated completion: {}",
                        estimate(&self.analysis, &self.settings)
                    ))
                    .color(TEAL),
                );
            });
        });
    }

    fn settings_panel(&mut self, ui: &mut egui::Ui) {
        match self.tab {
            Tab::Writer => self.writer(ui),
            Tab::Timing => timing_panel(ui, &mut self.settings),
            Tab::Humanization => humanization_panel(ui, &mut self.settings),
            Tab::Revisions => revision_panel(ui, &mut self.settings),
            Tab::Hotkeys => hotkey_panel(ui, &mut self.settings),
        }
    }

    fn controls(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let can_start = !self.text.is_empty() && self.status != "Typing";
            if ui
                .add_enabled(
                    can_start,
                    egui::Button::new(RichText::new("START  F6").strong()).fill(ORANGE),
                )
                .clicked()
            {
                self.analysis = TextAnalysis::parse(&self.text);
                let _ = self.commands.send(EngineCommand::Start(
                    self.text.clone(),
                    self.settings.clone(),
                ));
                self.status = "Typing".into();
            }
            let pause_label = if self.stats.paused {
                "RESUME  F7"
            } else {
                "PAUSE  F7"
            };
            if ui
                .add_enabled(
                    self.status == "Typing" || self.stats.paused,
                    egui::Button::new(pause_label),
                )
                .clicked()
            {
                let _ = self.commands.send(if self.stats.paused {
                    EngineCommand::Resume
                } else {
                    EngineCommand::Pause
                });
            }
            if ui
                .add_enabled(
                    self.status == "Typing" || self.stats.paused,
                    egui::Button::new("STOP  F8"),
                )
                .clicked()
            {
                let _ = self.commands.send(EngineCommand::Stop);
                self.status = "Stopped".into();
            }
            ui.separator();
            ui.label(
                RichText::new(format!(
                    "{} elapsed  /  {} remaining",
                    elapsed(&self.stats),
                    remaining(&self.stats, &self.settings)
                ))
                .color(MUTED),
            );
        });
    }
}

impl eframe::App for AutoWriteApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.receive_events();
        if self.last_save.elapsed() > Duration::from_secs(2) {
            self.settings.save();
            self.last_save = Instant::now();
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(BG).inner_margin(20.0))
            .show(ctx, |ui| {
                self.top_bar(ui);
                self.settings_panel(ui);
                self.controls(ui);
            });
        ctx.request_repaint_after(Duration::from_millis(100));
    }
}

fn configure_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = PANEL;
    style.visuals.window_fill = PANEL;
    style.visuals.override_text_color = Some(INK);
    style.visuals.widgets.noninteractive.bg_stroke =
        Stroke::new(1.0_f32, Color32::from_rgb(62, 67, 69));
    ctx.set_style(style);
}
fn stat_pair(ui: &mut egui::Ui, label: &str, value: String) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).color(MUTED));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(value).strong().color(INK));
        });
    });
}
fn progress(stats: &EngineStats) -> f32 {
    if stats.total == 0 {
        0.0
    } else {
        (stats.typed as f32 / stats.total as f32) * 100.0
    }
}
fn format_wpm(stats: &EngineStats) -> String {
    stats
        .started_at
        .map(|started| {
            format!(
                "{:.0}",
                stats.typed as f32 / 5.0 / started.elapsed().as_secs_f32().max(1.0) * 60.0
            )
        })
        .unwrap_or_else(|| "0".into())
}
fn estimate(analysis: &TextAnalysis, settings: &Settings) -> String {
    let seconds = if settings.use_target_time {
        settings.target_seconds
    } else {
        analysis.characters as f32 / (settings.wpm.max(1.0) * 5.0) * 60.0
    };
    format!("{}:{:02}", seconds as u64 / 60, seconds as u64 % 60)
}
fn elapsed(stats: &EngineStats) -> String {
    stats
        .started_at
        .map(|start| format!("{}s", start.elapsed().as_secs()))
        .unwrap_or_else(|| "0s".into())
}
fn remaining(stats: &EngineStats, settings: &Settings) -> String {
    let total = if settings.use_target_time {
        settings.target_seconds
    } else {
        stats.total as f32 / (settings.wpm.max(1.0) * 5.0) * 60.0
    };
    let used = stats
        .started_at
        .map(|start| start.elapsed().as_secs_f32())
        .unwrap_or(0.0);
    format!("{}s", (total - used).max(0.0) as u64)
}

fn timing_panel(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.heading("Timing");
    ui.label("Shape an average speed without flattening the rhythm.");
    ui.add(egui::Slider::new(&mut settings.wpm, 10.0..=180.0).text("Base WPM"));
    ui.add(egui::Slider::new(&mut settings.target_seconds, 10.0..=900.0).text("Target seconds"));
    ui.add(egui::Slider::new(&mut settings.speed_variation, 0.0..=0.7).text("Speed variation"));
    ui.add(egui::Slider::new(&mut settings.min_delay_ms, 1..=100).text("Minimum delay ms"));
    ui.add(egui::Slider::new(&mut settings.max_delay_ms, 100..=1500).text("Maximum delay ms"));
}
fn humanization_panel(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.heading("Humanization");
    ui.checkbox(&mut settings.thinking_enabled, "Thinking pauses");
    ui.add(
        egui::Slider::new(&mut settings.thinking_probability, 0.0..=0.2).text("Pause probability"),
    );
    ui.add(egui::Slider::new(&mut settings.thinking_min_ms, 100..=5000).text("Minimum pause ms"));
    ui.add(egui::Slider::new(&mut settings.thinking_max_ms, 500..=10000).text("Maximum pause ms"));
    ui.separator();
    ui.checkbox(&mut settings.mistakes_enabled, "Mistake simulation");
    ui.add(egui::Slider::new(&mut settings.mistake_rate, 0.0..=0.08).text("Mistake rate"));
    ui.add(egui::Slider::new(&mut settings.burst_probability, 0.0..=0.4).text("Burst probability"));
}
fn revision_panel(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.heading("Wording revisions");
    ui.checkbox(&mut settings.revision_enabled, "Enable small revisions");
    ui.add(egui::Slider::new(&mut settings.revision_probability, 0.0..=0.5).text("Probability"));
    ui.add(
        egui::Slider::new(&mut settings.revision_min_seconds, 1..=120).text("Earliest revision s"),
    );
    ui.add(
        egui::Slider::new(&mut settings.revision_max_seconds, 10..=600).text("Latest revision s"),
    );
    ui.add(egui::Slider::new(&mut settings.max_revisions, 0..=8).text("Maximum revisions"));
    ui.add(
        egui::Slider::new(&mut settings.revision_aggressiveness, 0.0..=1.0).text("Aggressiveness"),
    );
}
fn hotkey_panel(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.heading("Hotkeys");
    ui.label("Function-key defaults are reserved for the worker while Auto Write is open.");
    ui.add(egui::DragValue::new(&mut settings.hotkey_start).prefix("Start virtual key: "));
    ui.add(egui::DragValue::new(&mut settings.hotkey_pause).prefix("Pause virtual key: "));
    ui.add(egui::DragValue::new(&mut settings.hotkey_stop).prefix("Stop virtual key: "));
    ui.add(egui::Slider::new(&mut settings.countdown_seconds, 0..=10).text("Countdown seconds"));
}
