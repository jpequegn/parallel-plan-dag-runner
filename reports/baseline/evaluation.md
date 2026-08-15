# Sequential vs Parallel Evaluation

- Fixtures: 18
- Runs: 54
- Observed material break-even graph width (>10% speedup): 2

| Fixture | Width | Speedup | Parallel overhead (us) | Flawed correct |
|---|---:|---:|---:|---|
| research-width-1 | 1 | 1.23x | 6676 | false |
| research-width-2 | 2 | 1.19x | 13169 | false |
| research-width-3 | 3 | 1.88x | 6447 | false |
| research-width-4 | 4 | 1.49x | 16842 | false |
| research-width-6 | 6 | 2.27x | 12128 | false |
| risk-width-1 | 1 | 0.98x | 12958 | false |
| risk-width-2 | 2 | 1.47x | 8348 | false |
| risk-width-3 | 3 | 1.23x | 15223 | false |
| risk-width-5 | 5 | 2.08x | 10161 | false |
| data-width-1 | 1 | 0.83x | 16141 | false |
| data-width-2 | 2 | 1.10x | 12351 | false |
| data-width-4 | 4 | 2.12x | 8345 | false |
| data-width-5 | 5 | 1.74x | 16963 | false |
| data-width-6 | 6 | 2.47x | 9580 | false |
| endurance-width-1 | 1 | 1.00x | 9034 | false |
| endurance-width-2 | 2 | 1.04x | 16947 | false |
| endurance-width-3 | 3 | 1.35x | 13823 | false |
| endurance-width-6 | 6 | 2.02x | 13913 | false |

Parallelism pays when saved independent work exceeds scheduling overhead. The flawed mode intentionally omits one merge dependency; its failed verifier makes dependency extraction errors visible rather than producing a plausible final result.
