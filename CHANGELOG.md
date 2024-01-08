This changelog documents changes across multiple projects contained in this monorepo. Each project is released for every SQLSync version, even if the project has not changed. The reason for this decision is to simplify testing and debugging. Lockstep versioning will be relaxed as SQLSync matures.

# 0.3.0 - Jan 7 2023

- Moved the majority of functionality from `sqlsync-react` to `sqlsync-worker` to make it easier to add additional JS framework support. ([#38])
- Introduce Reducer trait, allowing non-Wasm reducers to be used with the Coordinator. ([#40]) Contribution by @matthewgapp.
- Allow SQLite to be directly modified in the coordinator via a new mutate_direct method. ([#43])
- Solid.js library ([#37]) Contribution by @matthewgapp.

# 0.2.0 - Dec 1 2023

- Reducer can now handle query errors ([#29])

# 0.1.0 - Oct 23 2023

- Initial release

[#29]: https://github.com/orbitinghail/sqlsync/pull/29
[#38]: https://github.com/orbitinghail/sqlsync/pull/38
[#40]: https://github.com/orbitinghail/sqlsync/pull/40
[#43]: https://github.com/orbitinghail/sqlsync/pull/43
[#37]: https://github.com/orbitinghail/sqlsync/pull/37
