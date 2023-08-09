use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{anyhow, Result};
use cadence::prelude::*;
use rayon::prelude::*;
use serde::Deserialize;
use tracing::{debug, error, info, warn};

mod config;
mod json;

use config::{check_stdout, format_variables, Backreference, CheckConfig, CheckStage};

pub struct CommandProbe {
    config: Vec<CheckConfig>,
    metrics: cadence::StatsdClient,
}

impl CommandProbe {
    pub fn new(config: PathBuf, metrics: cadence::StatsdClient) -> Self {
        let config = read_configuration(config);
        Self { config, metrics }
    }

    pub fn run_checks(&self) -> Result<()> {
        let passed = self
            .config
            .par_iter()
            .map(|c| self.run_check(c))
            .all(|boolean| boolean);
        if passed {
            info!("All checks passed.");
            self.increment_counter("probe.passed", None);
            Ok(())
        } else {
            error!("Some checks did not pass");
            self.increment_counter("probe.failed", None);
            Err(anyhow!("Failed"))
        }
    }

    fn run_check(&self, check: &CheckConfig) -> bool {
        let mut successful_stages = 0;
        let mut saved: HashMap<Backreference, String> = HashMap::new();
        for stage in check.stages.iter() {
            if self.run_stage(&check.test_name, stage, &mut saved).is_ok() {
                successful_stages += 1;
            }
        }
        if successful_stages == check.stages.len() {
            info!(check.test_name, status = "Check passed");
            self.increment_counter(
                "check.passed",
                Some(tags(vec![("test_name", &check.test_name)])),
            );
            true
        } else {
            error!(check.test_name, status = "Check failed",);
            self.increment_counter(
                "check.failed",
                Some(tags(vec![("test_name", &check.test_name)])),
            );
            false
        }
    }

    fn run_stage(
        &self,
        test_name: &str,
        stage: &CheckStage,
        saved: &mut HashMap<Backreference, String>,
    ) -> Result<()> {
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
                        self.increment_counter(
                            "stage.passed",
                            Some(tags(vec![("test_name", test_name), ("stage", &stage.name)])),
                        );
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

            if let Some(delay) = stage.delay_after {
                debug!(
                    test_name,
                    stage.name, "Sleeping {} seconds after check execution", delay
                );
                std::thread::sleep(std::time::Duration::from_secs(delay));
            }
        }

        self.increment_counter(
            "stage.failed",
            Some(tags(vec![("test_name", test_name), ("stage", &stage.name)])),
        );
        Err(anyhow!("Stage failed after {} retries", stage.max_retries))
    }

    fn increment_counter(&self, key: &str, tags: Option<HashMap<String, String>>) {
        let mut metric = self.metrics.incr_with_tags(key);
        let tags = tags.unwrap_or_default();
        for (key, value) in tags.iter() {
            metric = metric.with_tag(key, value);
        }
        metric.send()
    }
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

fn tags<T: std::fmt::Display>(t: Vec<(&str, T)>) -> HashMap<String, String> {
    let mut ret = HashMap::new();
    for (key, value) in t.into_iter() {
        ret.insert(key.to_string(), value.to_string());
    }
    ret
}
