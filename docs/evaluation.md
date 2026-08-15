# Evaluation Guide

The benchmark suite generates research, risk, data-transform, and endurance DAGs at widths 1 through 6. Every fixture runs in three modes: sequential, correct parallel, and parallel with one intentionally omitted merge dependency.

```bash
cargo run -p runner-cli -- evaluate \
  --fixtures benchmarks/fixtures.json \
  --output reports/latest
```

Outputs are JSON for automation, CSV for analysis, and Markdown for review. The checked-in `reports/baseline` run includes 18 fixtures and 54 executions.

## Interpretation

The material break-even rule is the smallest graph width above one where parallel execution is more than 10 percent faster than sequential execution. The baseline observed width 2. This is evidence for these deterministic, millisecond-scale fixtures, not a universal scheduler constant. Re-run on the target machine and with representative tool latency before choosing defaults.

`coordination_overhead_us` compares observed parallel wall time with the fixture's modeled critical path. `correct` checks the verified terminal output. Every flawed-dependency run should be incorrect and report a failed merge; a fast but incorrect run is a failed experiment.

Prioritize correctness, failed-merge detection, replay equivalence, and bounded tool calls before speedup. Timing rows can vary under system load, while those invariants should remain stable.
