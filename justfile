# List available recipes
default:
    @just --list

# Remove build artifacts
clean:
    cargo clean

# Type-check without building
check:
    cargo check

# Build debug binary
build:
    cargo build

# Format code
fmt:
    cargo fmt

# Check formatting without modifying files
fmt-check:
    cargo fmt -- --check

# Run clippy lints
clippy:
    cargo clippy -- -D warnings

# Run tests
test:
    cargo test

# Run full CI suite locally
ci: fmt-check clippy coverage

# Generate LCOV coverage report
coverage:
    cargo llvm-cov \
        --lcov \
        --ignore-filename-regex '(tests/|build\.rs)' \
        --output-path lcov.info
    @echo ""
    @echo "Coverage report written to lcov.info"
    @awk '/^LH:/{h+=substr($0,4)} /^LF:/{t+=substr($0,4)} END{printf "Overall: %d/%d lines (%.1f%%)\n",h,t,(h/t)*100}' lcov.info

# Generate HTML coverage report
coverage-html:
    cargo llvm-cov \
        --html \
        --ignore-filename-regex '(tests/|build\.rs)' \
        --output-dir coverage/
    @echo ""
    @echo "HTML report written to coverage/"

# Remove coverage artifacts
clean-coverage:
    cargo llvm-cov clean
    rm -rf lcov.info coverage/
