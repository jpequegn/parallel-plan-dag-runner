use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs,
    path::Path,
    time::Instant,
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::{
    AuthorityPolicy, EventKind, ExecutionError, ExecutionMode, Executor, FailurePolicy, InputSpec,
    Node, NodeRunner, Plan, PlanLimits, Provenance, ResolvedOutput, RunResult, RunStatus,
    ToolError, ValueType, VerifierSpec, canonical_digest,
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FixtureSpec {
    pub id: String,
    pub domain: String,
    pub width: usize,
    pub tail_depth: usize,
    pub delay_ms: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExperimentMode {
    Sequential,
    Parallel,
    FlawedDependency,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExperimentRecord {
    pub fixture_id: String,
    pub domain: String,
    pub mode: ExperimentMode,
    pub graph_width: usize,
    pub node_count: usize,
    pub wall_time_us: u64,
    pub critical_path_us: u64,
    pub tool_calls: usize,
    pub token_units: usize,
    pub failed_merges: usize,
    pub replans: usize,
    pub correct: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct FixtureComparison {
    pub fixture_id: String,
    pub graph_width: usize,
    pub parallel_speedup_milli: u64,
    pub coordination_overhead_us: i64,
    pub flawed_correct: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct EvaluationReport {
    pub fixture_count: usize,
    pub run_count: usize,
    pub break_even_width: Option<usize>,
    pub records: Vec<ExperimentRecord>,
    pub comparisons: Vec<FixtureComparison>,
}

#[derive(Debug, Error)]
pub enum EvaluationError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Csv(#[from] csv::Error),
    #[error(transparent)]
    Execution(#[from] ExecutionError),
    #[error("invalid fixture '{fixture}': {reason}")]
    InvalidFixture { fixture: String, reason: String },
}

pub struct ExperimentHarness;

impl ExperimentHarness {
    /// Load fixture specifications from JSON.
    ///
    /// # Errors
    /// Returns an I/O, JSON, or fixture validation error.
    pub fn load(path: impl AsRef<Path>) -> Result<Vec<FixtureSpec>, EvaluationError> {
        let fixtures: Vec<FixtureSpec> = serde_json::from_str(&fs::read_to_string(path)?)?;
        for fixture in &fixtures {
            if fixture.width == 0 || fixture.tail_depth == 0 || fixture.delay_ms == 0 {
                return Err(EvaluationError::InvalidFixture {
                    fixture: fixture.id.clone(),
                    reason: "width, tail_depth, and delay_ms must be positive".to_owned(),
                });
            }
        }
        Ok(fixtures)
    }

    /// Run every fixture in sequential, correct-parallel, and flawed-dependency modes.
    ///
    /// # Errors
    /// Returns an execution or metric conversion error.
    pub async fn run(fixtures: &[FixtureSpec]) -> Result<EvaluationReport, EvaluationError> {
        let mut records = Vec::with_capacity(fixtures.len() * 3);
        for fixture in fixtures {
            for mode in [
                ExperimentMode::Sequential,
                ExperimentMode::Parallel,
                ExperimentMode::FlawedDependency,
            ] {
                records.push(run_fixture(fixture, mode).await?);
            }
        }
        let comparisons = comparisons(fixtures, &records);
        let break_even_width = comparisons
            .iter()
            .filter(|comparison| {
                comparison.graph_width > 1 && comparison.parallel_speedup_milli > 1_100
            })
            .map(|comparison| comparison.graph_width)
            .min();
        Ok(EvaluationReport {
            fixture_count: fixtures.len(),
            run_count: records.len(),
            break_even_width,
            records,
            comparisons,
        })
    }

    /// Write machine-readable JSON/CSV and a human-readable Markdown report.
    ///
    /// # Errors
    /// Returns an I/O, JSON, or CSV serialization error.
    pub fn write(
        report: &EvaluationReport,
        output: impl AsRef<Path>,
    ) -> Result<(), EvaluationError> {
        let output = output.as_ref();
        fs::create_dir_all(output)?;
        fs::write(
            output.join("evaluation.json"),
            serde_json::to_string_pretty(report)?,
        )?;
        let mut csv = csv::Writer::from_path(output.join("evaluation.csv"))?;
        for record in &report.records {
            csv.serialize(record)?;
        }
        csv.flush()?;
        fs::write(output.join("evaluation.md"), markdown_report(report))?;
        Ok(())
    }
}

struct BenchmarkRunner {
    delay_ms: u64,
}

#[async_trait]
impl NodeRunner for BenchmarkRunner {
    async fn run_node(
        &self,
        _plan: &Plan,
        node: &Node,
        inputs: &BTreeMap<String, Value>,
    ) -> Result<ResolvedOutput, ToolError> {
        tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;
        let value = match node.tool.as_str() {
            "fixture_leaf" => Value::Number(Number::from(1)),
            "fixture_merge" => {
                let total = inputs.values().filter_map(Value::as_u64).sum::<u64>();
                Value::Number(Number::from(total))
            }
            "fixture_pass" => inputs
                .values()
                .next()
                .cloned()
                .ok_or_else(|| ToolError::Execution("pass node has no input".to_owned()))?,
            other => {
                return Err(ToolError::Execution(format!(
                    "unknown benchmark tool '{other}'"
                )));
            }
        };
        let digest = canonical_digest(&value);
        Ok(ResolvedOutput {
            value,
            provenance: Provenance {
                node_id: node.id.clone(),
                invocation_id: digest.clone(),
                tool_name: node.tool.clone(),
                request_digest: digest.clone(),
                response_digest: digest.clone(),
                content_digest: digest,
            },
        })
    }
}

async fn run_fixture(
    fixture: &FixtureSpec,
    mode: ExperimentMode,
) -> Result<ExperimentRecord, EvaluationError> {
    let flawed = mode == ExperimentMode::FlawedDependency;
    let plan = build_plan(fixture, flawed)?;
    let execution_mode = if mode == ExperimentMode::Sequential {
        ExecutionMode::Sequential
    } else {
        ExecutionMode::Parallel
    };
    let runner = BenchmarkRunner {
        delay_ms: fixture.delay_ms,
    };
    let started = Instant::now();
    let result = Executor::new(&runner)
        .mode(execution_mode)
        .execute(&plan, CancellationToken::new())
        .await?;
    let wall_time_us = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
    let critical_nodes = fixture.tail_depth.saturating_add(2);
    let critical_path_us = fixture
        .delay_ms
        .saturating_mul(1_000)
        .saturating_mul(u64::try_from(critical_nodes).unwrap_or(u64::MAX));
    Ok(ExperimentRecord {
        fixture_id: fixture.id.clone(),
        domain: fixture.domain.clone(),
        mode,
        graph_width: fixture.width,
        node_count: plan.nodes.len(),
        wall_time_us,
        critical_path_us,
        tool_calls: result
            .events
            .iter()
            .filter(|event| matches!(event, EventKind::ToolCall { .. }))
            .count(),
        token_units: token_units(&result),
        failed_merges: result
            .events
            .iter()
            .filter(|event| {
                matches!(event, EventKind::NodeFailed { node_id, .. } if node_id == "merge")
            })
            .count(),
        replans: result
            .events
            .iter()
            .filter(|event| matches!(event, EventKind::Replan { .. }))
            .count(),
        correct: is_correct(fixture, &result),
    })
}

fn build_plan(fixture: &FixtureSpec, flawed: bool) -> Result<Plan, EvaluationError> {
    let expected = u64::try_from(fixture.width).map_err(|_| EvaluationError::InvalidFixture {
        fixture: fixture.id.clone(),
        reason: "width does not fit in u64".to_owned(),
    })?;
    let mut nodes: Vec<_> = (0..fixture.width)
        .map(|index| {
            benchmark_node(
                &format!("leaf-{index:02}"),
                "fixture_leaf",
                &[],
                VerifierSpec::Always,
            )
        })
        .collect();
    let merge_width = if flawed {
        fixture.width.saturating_sub(1)
    } else {
        fixture.width
    };
    let merge_dependencies: Vec<_> = (0..merge_width)
        .map(|index| format!("leaf-{index:02}"))
        .collect();
    let merge_refs: Vec<_> = merge_dependencies.iter().map(String::as_str).collect();
    nodes.push(benchmark_node(
        "merge",
        "fixture_merge",
        &merge_refs,
        VerifierSpec::Equals {
            expected: Value::Number(Number::from(expected)),
        },
    ));
    let mut previous = "merge".to_owned();
    for index in 0..fixture.tail_depth {
        let id = format!("tail-{index:02}");
        nodes.push(benchmark_node(
            &id,
            "fixture_pass",
            &[previous.as_str()],
            VerifierSpec::Equals {
                expected: Value::Number(Number::from(expected)),
            },
        ));
        previous = id;
    }
    Ok(Plan {
        version: "v1".to_owned(),
        id: fixture.id.clone(),
        authority: AuthorityPolicy {
            tools: BTreeSet::from([
                "fixture_leaf".to_owned(),
                "fixture_merge".to_owned(),
                "fixture_pass".to_owned(),
            ]),
            capabilities: BTreeSet::from(["compute".to_owned()]),
        },
        limits: PlanLimits {
            max_concurrency: fixture.width.max(1),
            max_replans: 0,
            max_node_growth: 0,
            max_replan_wall_time_ms: 5_000,
        },
        nodes,
        final_verifier: None,
        final_output_schema: None,
    })
}

fn benchmark_node(id: &str, tool: &str, dependencies: &[&str], verifier: VerifierSpec) -> Node {
    Node {
        id: id.to_owned(),
        objective: format!("benchmark {id}"),
        dependencies: dependencies.iter().map(ToString::to_string).collect(),
        inputs: dependencies
            .iter()
            .map(|dependency| {
                (
                    (*dependency).to_owned(),
                    InputSpec::Reference {
                        node: (*dependency).to_owned(),
                        path: None,
                        value_type: ValueType::Number,
                    },
                )
            })
            .collect(),
        output: ValueType::Number,
        output_schema: None,
        tool: tool.to_owned(),
        authority: BTreeSet::from(["compute".to_owned()]),
        timeout_ms: 2_000,
        retry_budget: 0,
        verifier,
        failure_policy: FailurePolicy::Stop,
        degrade_value: None,
        immutable: false,
    }
}

fn is_correct(fixture: &FixtureSpec, result: &RunResult) -> bool {
    let final_id = format!("tail-{:02}", fixture.tail_depth - 1);
    result.status == RunStatus::Succeeded
        && result
            .outputs
            .get(&final_id)
            .and_then(|output| output.value.as_u64())
            == u64::try_from(fixture.width).ok()
}

fn token_units(result: &RunResult) -> usize {
    let bytes = result
        .events
        .iter()
        .filter(|event| {
            matches!(
                event,
                EventKind::ToolCall { .. } | EventKind::ToolResponse { .. }
            )
        })
        .map(|event| serde_json::to_vec(event).map_or(0, |value| value.len()))
        .sum::<usize>();
    bytes.div_ceil(4)
}

fn comparisons(fixtures: &[FixtureSpec], records: &[ExperimentRecord]) -> Vec<FixtureComparison> {
    fixtures
        .iter()
        .filter_map(|fixture| {
            let sequential = records.iter().find(|record| {
                record.fixture_id == fixture.id && record.mode == ExperimentMode::Sequential
            })?;
            let parallel = records.iter().find(|record| {
                record.fixture_id == fixture.id && record.mode == ExperimentMode::Parallel
            })?;
            let flawed = records.iter().find(|record| {
                record.fixture_id == fixture.id && record.mode == ExperimentMode::FlawedDependency
            })?;
            let parallel_speedup_milli = if parallel.wall_time_us == 0 {
                0
            } else {
                sequential.wall_time_us.saturating_mul(1_000) / parallel.wall_time_us
            };
            Some(FixtureComparison {
                fixture_id: fixture.id.clone(),
                graph_width: fixture.width,
                parallel_speedup_milli,
                coordination_overhead_us: i64::try_from(parallel.wall_time_us).unwrap_or(i64::MAX)
                    - i64::try_from(parallel.critical_path_us).unwrap_or(i64::MAX),
                flawed_correct: flawed.correct,
            })
        })
        .collect()
}

fn markdown_report(report: &EvaluationReport) -> String {
    let break_even = report
        .break_even_width
        .map_or_else(|| "not observed".to_owned(), |width| width.to_string());
    let mut markdown = format!(
        "# Sequential vs Parallel Evaluation\n\n- Fixtures: {}\n- Runs: {}\n- Observed material break-even graph width (>10% speedup): {}\n\n| Fixture | Width | Speedup | Parallel overhead (us) | Flawed correct |\n|---|---:|---:|---:|---|\n",
        report.fixture_count, report.run_count, break_even
    );
    for comparison in &report.comparisons {
        writeln!(
            markdown,
            "| {} | {} | {}.{:02}x | {} | {} |",
            comparison.fixture_id,
            comparison.graph_width,
            comparison.parallel_speedup_milli / 1_000,
            (comparison.parallel_speedup_milli % 1_000) / 10,
            comparison.coordination_overhead_us,
            comparison.flawed_correct
        )
        .expect("writing to a String cannot fail");
    }
    markdown.push_str(
        "\nParallelism pays when saved independent work exceeds scheduling overhead. The flawed mode intentionally omits one merge dependency; its failed verifier makes dependency extraction errors visible rather than producing a plausible final result.\n",
    );
    markdown
}
