//! What a scan finds on an invented machine.
//!
//! ```text
//! cargo run -p wave-core --example dump-scan -- <fixture.json> "<program name>"
//! ```
//!
//! The same code path the app runs, printed instead of applied — which is also
//! the quickest way to see why a folder was or was not proposed.

use wave_core::fixture::FixtureProvider;
use wave_core::{inventory, plan, scan};

fn main() {
    let mut arguments = std::env::args().skip(1);
    let (Some(fixture_path), Some(program_name)) = (arguments.next(), arguments.next()) else {
        eprintln!("usage: dump-scan <fixture.json> <program name>");
        std::process::exit(2);
    };

    let json = std::fs::read_to_string(&fixture_path).expect("the fixture should be readable");
    let provider = FixtureProvider::from_json(&json).expect("the fixture should parse");

    let inventory = inventory::collect(&provider);
    let Some(program) =
        inventory.programs.iter().find(|program| program.name.eq_ignore_ascii_case(&program_name))
    else {
        eprintln!("no program called {program_name:?} in {fixture_path}");
        std::process::exit(1);
    };

    let scan = scan::scan(&provider, &inventory, program);
    println!("{}", serde_json::to_string_pretty(&scan).expect("the scan should serialize"));

    let plan = plan::build(&provider, &scan, &plan::Selection::recommended(&scan));
    println!("{}", serde_json::to_string_pretty(&plan).expect("the plan should serialize"));
}
