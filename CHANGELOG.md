# Changelog

All notable changes to Trust Deeds are documented here.

## [0.1.0.0] - 2026-08-11

### Added

- See current, due, and late mortgage counts at a glance, with the 10th retained as the payment grace deadline.
- Distinguish pending checks with amber treatment throughout overview and payment history views.

### Changed

- Prioritize trust balance and payment standing on the Mortgage Office overview, with portfolio value and yield moved into a compact secondary row.
- Withhold payment standing when imported data is stale while keeping fresh degraded portfolio summaries available with their warning.
- Keep the active-loan portfolio accurate when loans disappear from a complete Mortgage Office capture.
- Run client interaction tests in continuous integration alongside the existing build and Rust verification.

### Removed

- Remove the overview sync summary and year-to-date interest metric from the primary portfolio view.
