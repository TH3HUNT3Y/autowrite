use crate::config::settings::Settings;
use rand::Rng;

pub fn delay_for(character: char, settings: &Settings, rng: &mut impl Rng, target_wpm: f32) -> u64 {
    let variation =
        rng.random_range(1.0 - settings.speed_variation..=1.0 + settings.speed_variation);
    let mut delay = (60_000.0 / (target_wpm.max(1.0) * 5.0)) * variation;
    if rng.random_bool(settings.burst_probability.clamp(0.0, 1.0) as f64) {
        delay *= 0.55;
    }
    delay += punctuation_pause(character, settings, rng);
    delay.clamp(settings.min_delay_ms as f32, settings.max_delay_ms as f32) as u64
}

fn punctuation_pause(character: char, settings: &Settings, rng: &mut impl Rng) -> f32 {
    let multiplier = rng.random_range(0.75..=1.25);
    let base = match character {
        ',' => 300.0,
        '.' | '!' | '?' => 500.0,
        ':' | ';' => 350.0,
        '\n' => settings.punctuation_pause_ms as f32,
        _ => 0.0,
    };
    base * multiplier
}
