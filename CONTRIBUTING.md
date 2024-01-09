# Contributing to SQLSync

This document attempts to explain how to work on SQLSync itself. Buckle up, it's pretty rough and is changing fast.

### Dependencies

- [Just](https://github.com/casey/just)
- [Rust](https://www.rust-lang.org/)
- [wasm-pack](https://rustwasm.github.io/wasm-pack/)
- [node.js](https://nodejs.org/en)
- [pnpm](https://pnpm.io/)
- [llvm](https://llvm.org/)
- [clang](https://clang.llvm.org/)

### Build Wasm artifacts

```bash
just run-with-prefix 'wasm-'
just wasm-demo-reducer --release
just package-sqlsync-worker dev
```

### Local Coordinator

> [!WARNING]
> Currently this seems to require modifying the wrangler.toml config file to point at your own Cloudflare buckets (even though they aren't being used). Work is underway to replace the local coordinator with a wrangler agnostic alternative optimized for local development.

```bash
cd demo/cloudflare-backend
pnpm i
pnpm dev

# then in another shell
just upload-demo-reducer release local
```

### Local Todo Demo

```bash
cd demo/frontend
pnpm i
pnpm dev
```

Then go to http://localhost:5173

### Run some tests

These tests are useful for learning more about how SQLSync works.

```bash
just unit-test
just test-end-to-end-local
just test-end-to-end-local-net
```

### Submitting a pull request

When submitting a pull request, it's appreciated if you run `just lint` as well as the above tests before each change. These commands also will run via GitHub actions which will be enabled on your PR once it's been reviewed. Thanks for your contributions!

## Community & Contributing

If you are interested in contributing to SQLSync, please [join the Discord community][discord] and let us know what you want to build. All contributions will be held to a high standard, and are more likely to be accepted if they are tied to an existing task and agreed upon specification.

[![Join the SQLSync Community](https://discordapp.com/api/guilds/1149205110262595634/widget.png?style=banner2)][discord]

[discord]: https://discord.gg/etFk2N9nzC
