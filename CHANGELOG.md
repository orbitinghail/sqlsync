This changelog documents changes across multiple projects contained in this monorepo. Each project is released for every SQLSync version, even if the project has not changed. The reason for this decision is to simplify testing and debugging. Lockstep versioning will be relaxed as SQLSync matures.

# Pending Changes

- Moved the majority of functionality from `sqlsync-react` to `sqlsync-worker` to make it easier to add additional JS framework support. ([#38])

# 0.2.0 - Dec 1 2023

- Reducer can now handle query errors ([#29])

# 0.1.0 - Oct 23 2023

- Initial release

[#38]: https://github.com/orbitinghail/sqlsync/pull/38
[#29]: https://github.com/orbitinghail/sqlsync/pull/29