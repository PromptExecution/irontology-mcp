# ── irontology-mcp — ontology/graph MCP server lifecycle ──────────────────
#
# Just module syntax:
#   In parent justfile:  mod irontology 'vendor/irontology-mcp/irontology.just'
#   Invocation:          just irontology build
#   List recipes:        just --list irontology

# Build irontology-mcp from submodule
build:
    cargo build --release --manifest-path Cargo.toml

# Run tests
test:
    cargo test --manifest-path Cargo.toml

# Update submodule to latest upstream
update:
    git -C .. submodule update --remote vendor/irontology-mcp
