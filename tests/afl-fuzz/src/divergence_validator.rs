use std::{env, ffi::OsStr, fs, path::Path, process};

use zksync_vm2_afl_fuzz::{
    scenario::Scenario, validate_scenario, validate_testcase, ValidationReport, STATUS_DIVERGENCE,
    STATUS_ERROR, STATUS_MATCH,
};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: vm2-divergence-validator <scenario.yaml|json|afl-testcase-file>");
        eprintln!();
        eprintln!(
            "Executes one vm2 audit scenario or AFL testcase against zk_evm and reports JSON."
        );
        process::exit(2);
    }

    let path = Path::new(&args[1]);
    let report = match fs::read(path) {
        Ok(bytes) if is_scenario_file(path) => match parse_scenario(path, &bytes) {
            Ok(scenario) => validate_scenario(bytes.len(), scenario),
            Err(err) => ValidationReport::error(bytes.len(), err),
        },
        Ok(bytes) => validate_testcase(&bytes),
        Err(err) => ValidationReport::error(0, format!("failed to read testcase file: {err}")),
    };

    let json = serde_json::to_string_pretty(&report).expect("failed to serialize report");
    println!("{json}");

    match report.status.as_str() {
        STATUS_MATCH => process::exit(0),
        STATUS_DIVERGENCE => process::exit(1),
        STATUS_ERROR => process::exit(2),
        _ => process::exit(2),
    }
}

fn is_scenario_file(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        extension == OsStr::new("yaml")
            || extension == OsStr::new("yml")
            || extension == OsStr::new("json")
    })
}

fn parse_scenario(path: &Path, bytes: &[u8]) -> Result<Scenario, String> {
    let content = std::str::from_utf8(bytes)
        .map_err(|err| format!("scenario file is not valid UTF-8: {err}"))?;
    if path
        .extension()
        .is_some_and(|extension| extension == OsStr::new("json"))
    {
        serde_json::from_str(content).map_err(|err| format!("failed to parse scenario JSON: {err}"))
    } else {
        serde_yaml::from_str(content).map_err(|err| format!("failed to parse scenario YAML: {err}"))
    }
}
