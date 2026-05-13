# Changelog

## v0.1.0 - 2026-05-12

### Added
- Scaffold agentic-rust-kit
- Add jig CLI tool and migrate to jig.sh branding
- Add template mode support for local git templates
- Add agent planning and workflow infrastructure
- Add state-summary tool and enhance receipts filtering
- Add block-managed root AGENTS.md to preserve repo-specific content during adoption

### Fixed
- Address extraction review findings
- Persist full copier template ref
- Unify kit config and update flow
- Make template source normalization safe

### Changed
- Split bootstrap module into separate concerns
- Split state module into separate concerns
- Extract tool definitions and remove memory tools from contract
- Extract bootstrap module concerns into separate files
- Improve encapsulation in template_source module
- Extract request parsing into runtime/requests module
- Extract work dispatch logic and improve runtime architecture
- Improve type safety, reduce bootstrap test cost, and organize runtime modules
- Move MCP tests to tests/mcp module and add Cargo.toml metadata

### Documentation
- Make copier update example noninteractive
- Distinguish recopy from update
- Make recopy noninteractive

### Tests
- Make fixture update check clean
- Fix update assertion
- Drop pyyaml dependency
- Add fixture infrastructure and agent documentation
- Refactor receipt creation and add plan state validation tests

### Other
- Improve work gate validation and receipt tracking
- Settle Cargo workspace in fixtures and document gate evidence requirements
- Add release script and normalize jig-sh package name
