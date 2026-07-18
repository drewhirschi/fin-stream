# Trust Deeds

The repository-root application is the NextRS/React/libSQL implementation. See `AGENTS.md` for the current stack, commands, structure, mobile requirements, formatting rules, and deployment model.

Implementation plans remain under `docs/plans/`. Completed plans are retained as design history even when they describe the retired PostgreSQL/Coolify application.

The production target is Vercel with Turso and private S3-compatible storage. The old root Axum/PostgreSQL runtime, Docker image, and Coolify CI pipeline have been removed; `CUTOVER.md` remains the operator migration and rollback record.
