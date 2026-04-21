use crate::presets::{ClickSequence, PresetStore, SequenceStepType};

const MAX_PLAN_DEPTH: usize = 32;
const MAX_PLAN_STEPS: usize = 10_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationSeverity {
    Error,
    Warning,
}

#[derive(Clone, Debug)]
pub struct ValidationIssue {
    pub severity: ValidationSeverity,
    pub message: String,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct PlannedStep {
    pub click_name: String,
    pub min_x: i32,
    pub max_x: i32,
    pub min_y: i32,
    pub max_y: i32,
    pub button_type: String,
    pub min_interval_secs: f32,
    pub max_interval_secs: f32,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct CompiledRunPlan {
    pub sequence_name: String,
    pub repetitions: u32,
    pub steps: Vec<PlannedStep>,
    pub estimated_duration_ms: u64,
    pub warnings: Vec<ValidationIssue>,
}

#[derive(Clone, Debug)]
pub struct ValidationReport {
    pub issues: Vec<ValidationIssue>,
    pub plan: Option<CompiledRunPlan>,
}

impl ValidationReport {
    pub fn has_errors(&self) -> bool {
        self.issues
            .iter()
            .any(|issue| issue.severity == ValidationSeverity::Error)
    }
}

pub fn compile_run_plan(
    store: &PresetStore,
    sequence_name: &str,
    repetitions: u32,
) -> ValidationReport {
    let mut issues = Vec::new();

    if repetitions == 0 {
        issues.push(ValidationIssue {
            severity: ValidationSeverity::Error,
            message: "Repetitions must be at least 1.".to_string(),
        });
    }

    let sequence = match store.get_sequence(sequence_name) {
        Some(sequence) => sequence,
        None => {
            issues.push(ValidationIssue {
                severity: ValidationSeverity::Error,
                message: format!("Sequence '{}' was not found.", sequence_name),
            });
            return ValidationReport { issues, plan: None };
        }
    };

    let mut steps = Vec::new();
    let mut stack = vec![sequence.name.clone()];
    compile_sequence_inner(
        store,
        sequence,
        &mut stack,
        0,
        &mut steps,
        &mut issues,
    );

    if steps.is_empty() {
        issues.push(ValidationIssue {
            severity: ValidationSeverity::Error,
            message: format!("Sequence '{}' has no executable click steps.", sequence_name),
        });
    }

    if issues.iter().any(|issue| issue.severity == ValidationSeverity::Error) {
        return ValidationReport { issues, plan: None };
    }

    let estimated_step_ms: u64 = steps
        .iter()
        .map(|step| {
            let interval_ms = ((step.min_interval_secs + step.max_interval_secs) * 500.0)
                .max(0.0) as u64;
            interval_ms + 250
        })
        .sum();

    let estimated_duration_ms = estimated_step_ms.saturating_mul(repetitions as u64);
    let warnings = issues
        .iter()
        .filter(|issue| issue.severity == ValidationSeverity::Warning)
        .cloned()
        .collect();

    ValidationReport {
        issues,
        plan: Some(CompiledRunPlan {
            sequence_name: sequence.name.clone(),
            repetitions,
            steps,
            estimated_duration_ms,
            warnings,
        }),
    }
}

fn compile_sequence_inner(
    store: &PresetStore,
    sequence: &ClickSequence,
    stack: &mut Vec<String>,
    depth: usize,
    steps: &mut Vec<PlannedStep>,
    issues: &mut Vec<ValidationIssue>,
) {
    if depth > MAX_PLAN_DEPTH {
        issues.push(ValidationIssue {
            severity: ValidationSeverity::Error,
            message: format!(
                "Sequence nesting exceeded the safety limit of {} levels.",
                MAX_PLAN_DEPTH
            ),
        });
        return;
    }

    for step in &sequence.steps {
        if steps.len() >= MAX_PLAN_STEPS {
            issues.push(ValidationIssue {
                severity: ValidationSeverity::Error,
                message: format!(
                    "Expanded run plan exceeded the safety limit of {} steps.",
                    MAX_PLAN_STEPS
                ),
            });
            return;
        }

        match step {
            SequenceStepType::Click {
                click_name,
                min_interval,
                max_interval,
            } => {
                let click = match store.get_click(click_name) {
                    Some(click) => click,
                    None => {
                        issues.push(ValidationIssue {
                            severity: ValidationSeverity::Error,
                            message: format!("Click '{}' was not found.", click_name),
                        });
                        continue;
                    }
                };

                if click.min_x >= click.max_x || click.min_y >= click.max_y {
                    issues.push(ValidationIssue {
                        severity: ValidationSeverity::Error,
                        message: format!(
                            "Click '{}' has invalid bounds: X({}-{}) Y({}-{}).",
                            click.name, click.min_x, click.max_x, click.min_y, click.max_y
                        ),
                    });
                    continue;
                }

                let (min_interval_secs, max_interval_secs) = normalize_delay(
                    *min_interval,
                    *max_interval,
                    &click.name,
                    issues,
                );

                steps.push(PlannedStep {
                    click_name: click.name.clone(),
                    min_x: click.min_x,
                    max_x: click.max_x,
                    min_y: click.min_y,
                    max_y: click.max_y,
                    button_type: click.button_type.clone(),
                    min_interval_secs,
                    max_interval_secs,
                });
            }
            SequenceStepType::Subsequence { sequence_name } => {
                if stack.iter().any(|name| name == sequence_name) {
                    let mut cycle = stack.clone();
                    cycle.push(sequence_name.clone());
                    issues.push(ValidationIssue {
                        severity: ValidationSeverity::Error,
                        message: format!("Recursive subsequence detected: {}", cycle.join(" -> ")),
                    });
                    continue;
                }

                let sub_sequence = match store.get_sequence(sequence_name) {
                    Some(sequence) => sequence,
                    None => {
                        issues.push(ValidationIssue {
                            severity: ValidationSeverity::Error,
                            message: format!("Sequence '{}' was not found.", sequence_name),
                        });
                        continue;
                    }
                };

                stack.push(sequence_name.clone());
                compile_sequence_inner(store, sub_sequence, stack, depth + 1, steps, issues);
                stack.pop();
            }
        }
    }
}

fn normalize_delay(
    min_interval: f32,
    max_interval: f32,
    click_name: &str,
    issues: &mut Vec<ValidationIssue>,
) -> (f32, f32) {
    let min_secs = min_interval.max(0.0);
    let max_secs = max_interval.max(0.0);

    if min_secs == 0.0 && max_secs == 0.0 {
        issues.push(ValidationIssue {
            severity: ValidationSeverity::Warning,
            message: format!("Click '{}' has a zero delay range.", click_name),
        });
    }

    if min_secs > max_secs {
        issues.push(ValidationIssue {
            severity: ValidationSeverity::Warning,
            message: format!(
                "Click '{}' had min delay greater than max delay; the values were normalized.",
                click_name
            ),
        });
        (max_secs, min_secs)
    } else {
        (min_secs, max_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets::{Click, PresetStore};

    #[test]
    fn compile_plan_flattens_subsequences() {
        let mut store = PresetStore::default();
        store.upsert_click(Click::new("A".to_string(), 10, 20, 10, 20));
        store.upsert_click(Click::new("B".to_string(), 30, 40, 30, 40));
        store.upsert_sequence(ClickSequence {
            name: "Child".to_string(),
            steps: vec![SequenceStepType::Click {
                click_name: "B".to_string(),
                min_interval: 0.5,
                max_interval: 1.0,
            }],
        });
        store.upsert_sequence(ClickSequence {
            name: "Root".to_string(),
            steps: vec![
                SequenceStepType::Click {
                    click_name: "A".to_string(),
                    min_interval: 0.5,
                    max_interval: 1.0,
                },
                SequenceStepType::Subsequence {
                    sequence_name: "Child".to_string(),
                },
            ],
        });

        let report = compile_run_plan(&store, "Root", 2);
        assert!(!report.has_errors());
        let plan = report.plan.expect("expected compiled plan");
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.repetitions, 2);
    }

    #[test]
    fn compile_plan_rejects_recursive_sequences() {
        let mut store = PresetStore::default();
        store.upsert_sequence(ClickSequence {
            name: "Loop".to_string(),
            steps: vec![SequenceStepType::Subsequence {
                sequence_name: "Loop".to_string(),
            }],
        });

        let report = compile_run_plan(&store, "Loop", 1);
        assert!(report.has_errors());
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.message.contains("Recursive subsequence detected")));
    }

    #[test]
    fn compile_plan_rejects_invalid_click_bounds() {
        let mut store = PresetStore::default();
        store.upsert_click(Click::new("Bad".to_string(), 20, 20, 10, 30));
        store.upsert_sequence(ClickSequence {
            name: "Root".to_string(),
            steps: vec![SequenceStepType::Click {
                click_name: "Bad".to_string(),
                min_interval: 0.5,
                max_interval: 1.0,
            }],
        });

        let report = compile_run_plan(&store, "Root", 1);
        assert!(report.has_errors());
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.message.contains("invalid bounds")));
    }
}
