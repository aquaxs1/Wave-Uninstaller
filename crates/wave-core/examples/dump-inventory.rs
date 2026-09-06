//! Print the inventory a fixture produces, as the UI would receive it.
//!
//! Useful for eyeballing a merge result without a Windows machine, and for
//! feeding the real backend shape into a frontend running in a browser:
//!
//! ```text
//! cargo run -p wave-core --example dump-inventory -- crates/wave-core/tests/fixtures/typical_machine.json
//! ```

use std::process::ExitCode;

use wave_core::fixture::FixtureProvider;

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: dump-inventory <fixture.json>");
        return ExitCode::FAILURE;
    };

    let json = match std::fs::read_to_string(&path) {
        Ok(json) => json,
        Err(error) => {
            eprintln!("{path}: {error}");
            return ExitCode::FAILURE;
        }
    };

    let provider = match FixtureProvider::from_json(&json) {
        Ok(provider) => provider,
        Err(error) => {
            eprintln!("{path}: {error}");
            return ExitCode::FAILURE;
        }
    };

    let inventory = wave_core::collect(&provider);
    println!("{}", serde_json::to_string_pretty(&inventory).expect("inventory is serializable"));
    ExitCode::SUCCESS
}
