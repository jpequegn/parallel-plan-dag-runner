use runner_core::{ExperimentHarness, ExperimentMode};

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../benchmarks/fixtures.json"
);

#[tokio::test]
async fn compares_all_fixtures_across_three_modes() {
    let fixtures = ExperimentHarness::load(FIXTURES).expect("load fixtures");
    assert_eq!(fixtures.len(), 18);
    let report = ExperimentHarness::run(&fixtures)
        .await
        .expect("run experiment");
    assert_eq!(report.fixture_count, 18);
    assert_eq!(report.run_count, 54);
    assert_eq!(report.comparisons.len(), 18);
    assert!(report.records.iter().all(|record| record.wall_time_us > 0));
    assert!(report.records.iter().all(|record| record.tool_calls > 0));
    assert!(report.records.iter().all(|record| {
        match record.mode {
            ExperimentMode::Sequential | ExperimentMode::Parallel => record.correct,
            ExperimentMode::FlawedDependency => !record.correct && record.failed_merges == 1,
        }
    }));
    assert!(
        report
            .comparisons
            .iter()
            .all(|comparison| !comparison.flawed_correct)
    );
}

#[tokio::test]
async fn writes_json_csv_and_markdown_contracts() {
    let fixtures = ExperimentHarness::load(FIXTURES).expect("load fixtures");
    let report = ExperimentHarness::run(&fixtures[..2])
        .await
        .expect("run experiment");
    let directory = tempfile::tempdir().expect("temporary directory");
    ExperimentHarness::write(&report, directory.path()).expect("write report");
    let json = std::fs::read_to_string(directory.path().join("evaluation.json")).expect("JSON");
    let csv = std::fs::read_to_string(directory.path().join("evaluation.csv")).expect("CSV");
    let markdown =
        std::fs::read_to_string(directory.path().join("evaluation.md")).expect("Markdown");
    assert!(json.contains("break_even_width"));
    assert!(csv.starts_with("fixture_id,domain,mode,graph_width"));
    assert!(markdown.contains("Observed material break-even graph width"));
    assert!(markdown.contains("Flawed correct"));
}
