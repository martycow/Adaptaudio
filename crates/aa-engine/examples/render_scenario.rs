//! Renders a scenario of the Cedar Station demo to a WAV file.
//!
//! `cargo run --release -p aa-engine --example render_scenario -- "Full night" night.wav`

use aa_engine::LoadedProject;
use aa_engine::offline::render_scenario;
use aa_engine::samples::write_wav_stereo;

fn main() {
    let mut args = std::env::args().skip(1);
    let name = args.next().unwrap_or_else(|| "Full night".into());
    let out = args.next().unwrap_or_else(|| "scenario.wav".into());
    let base = LoadedProject::without_samples(aa_core::demo::cedar_station());
    let Some(scenario) = base
        .project
        .scenarios
        .iter()
        .find(|s| s.name == name)
        .cloned()
    else {
        eprintln!("no scenario named {name:?}");
        std::process::exit(1);
    };
    let seconds = scenario.duration() + 8.0;
    let (audio, _) = render_scenario(&base, &scenario, seconds, 48_000.0);
    let peak = audio.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    match write_wav_stereo(std::path::Path::new(&out), &audio, 48_000) {
        Ok(()) => println!("{out}: {seconds:.0} s, peak {peak:.2}"),
        Err(e) => {
            eprintln!("can't write {out}: {e}");
            std::process::exit(1);
        }
    }
}
