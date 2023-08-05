use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{anyhow, Result};
use rayon::prelude::*;
use serde::Deserialize;
use tracing::{debug, error, info, warn};

mod config;
mod json;

use config::{check_stdout, format_variables, Backreference, CheckConfig, CheckStage};

pub fn run_checks_from_file(p: PathBuf) -> Result<()> {
    let checks = read_configuration(p);
    let passed = checks.par_iter().map(run_check).all(|boolean| boolean);
    if passed {
        info!("All checks passed.");
        Ok(())
    } else {
        error!("Some checks did not pass");
        Err(anyhow!("Failed"))
    }
}

fn run_check(check: &CheckConfig) -> bool {
    let mut successful_stages = 0;
    let mut saved: HashMap<Backreference, String> = HashMap::new();
    for stage in check.stages.iter() {
        if run_stage(&check.test_name, stage, &mut saved).is_ok() {
            successful_stages += 1;
        }
    }
    if successful_stages == check.stages.len() {
        info!(check.test_name, status = "Check passed");
        true
    } else {
        error!(check.test_name, status = "Check failed",);
        false
    }
}

fn run_stage(
    test_name: &str,
    stage: &CheckStage,
    saved: &mut HashMap<Backreference, String>,
) -> Result<(), Box<dyn std::error::Error>> {
    for attempt in 0..stage.max_retries {
        if let Some(delay) = stage.delay_before {
            debug!(
                test_name,
                stage.name, "Sleeping {} seconds before check execution", delay
            );
            std::thread::sleep(std::time::Duration::from_secs(delay));
        }

        match execute_command(&stage.command, saved) {
            Ok(stdout) => {
                let matched = check_stdout(stage, test_name, &stdout, saved);
                if matched {
                    info!(test_name, stage.name, status = "Stage passed");
                    return Ok(());
                } else {
                    warn!(
                        test_name,
                        stage.name,
                        status = "Output does not match expectation",
                        attempt = attempt
                    );
                }
            }
            Err(err) => {
                warn!(test_name, stage.name, status = "Stage failed", error = %err)
            }
        };
    }
    Err(format!("Stage failed after {} retries", stage.max_retries).into())
}

fn execute_command(command: &str, context: &HashMap<Backreference, String>) -> Result<String> {
    let arg = format_variables(command, context);
    let output = Command::new("sh").arg("-c").arg(arg).output();

    match output {
        Ok(output) => {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                Ok(stdout)
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                Err(anyhow!("Command execution failed: {}", stderr))
            }
        }
        Err(err) => Err(anyhow!("Error executing command: {}", err)),
    }
}

fn read_configuration(p: PathBuf) -> Vec<CheckConfig> {
    let yaml_content = fs::read_to_string(p).expect("Error reading config file");
    serde_yaml::Deserializer::from_str(&yaml_content)
        .map(|i| CheckConfig::deserialize(i).unwrap())
        .collect()
}
