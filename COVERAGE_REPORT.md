# Coverage Report

Generated: 2026-02-25 11:20:19 UTC

## Scope

- Test types: unit tests (`src/lib.rs` harness) + integration tests (`tests/integration_tests.rs`)
- Coverage workflow uses: `cargo test`, `llvm-profdata`, `llvm-cov`
- Container-based docker tests were not included (feature-gated)

## Test execution result

- Unit tests: 192 passed, 0 failed
- Integration tests: 56 passed, 0 failed

## Coverage summary (project files)

- Regions coverage: 82.11%
- Functions coverage: 82.93%
- Lines coverage: 80.73%

### Source only (`src/`)

- Lines: 12878
- Missed lines: 2828
- Line coverage: 78.04%

### Integration test code only (`tests/integration/`)

- Lines: 1801
- Missed lines: 1
- Line coverage: 99.94%

## Lowest covered source files (by line coverage)

| File | Line coverage |
|---|---|
| `src/web/admin.rs` | 32.67% |
| `src/db/models.rs` | 42.86% |
| `src/lib.rs` | 42.86% |
| `src/opds/feeds.rs` | 45.19% |
| `src/web/auth.rs` | 52.19% |
| `src/db/mod.rs` | 66.46% |
| `src/opds/covers.rs` | 70.28% |
| `src/scheduler.rs` | 71.20% |
| `src/scanner/mod.rs` | 73.74% |
| `src/scanner/parsers/mobi.rs` | 74.63% |
