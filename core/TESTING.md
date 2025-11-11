# Testing

## Test Categories

- **Standard tests**: Unit and integration tests (~138 tests)
- **Property tests**: Proptest-based tests in `*_prop.rs` files (3 tests)
- **Load tests**: Performance tests in `load_tests.rs` (1 test)

## Running Tests

Standard tests (debug mode):
```bash
cargo nextest run --workspace
```

Property and load tests (release mode only):
```bash
cargo nextest run --workspace --release -E 'binary(*_prop) + binary(load_tests)'
```

## CI Configuration

CI runs two parallel jobs:
- **Standard**: Debug mode on Ubuntu and macOS
- **Optimized**: Release mode on Ubuntu only, filters to `*_prop` and `load_tests` binaries

Property and load tests always run with optimizations for reasonable execution time and meaningful performance data.
