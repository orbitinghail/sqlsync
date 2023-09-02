default:
    @just --choose
build: (run-with-prefix 'wasm-')
    cargo build

run-with-prefix prefix:
    #!/usr/bin/env bash
    set -euo pipefail
    all_tasks=$(just --summary)

    for task in $all_tasks; do
        if [[ $task == {{prefix}}* ]]; then
            just $task
        fi
    done

unit-test:
    cargo test

wasm-sqlsync-reducer-guest:
    cargo build --target wasm32-unknown-unknown --example guest

wasm-worker-test-reducer:
    cargo build --target wasm32-unknown-unknown --package test-reducer

wasm-sqlsync-worker +FLAGS='--dev':
    cd lib/sqlsync-worker/sqlsync-worker-crate && wasm-pack build --target web {{FLAGS}}

wasm-demo-reducer *FLAGS:
    cargo build --target wasm32-unknown-unknown --package demo-reducer {{FLAGS}}

wasm-counter-reducer:
    cargo build --target wasm32-unknown-unknown --example counter-reducer

wasm-task-reducer:
    cargo build --target wasm32-unknown-unknown --example task-reducer

test-end-to-end-local: wasm-task-reducer
    RUST_BACKTRACE=1 cargo run --example end-to-end-local

test-end-to-end-local-net: wasm-counter-reducer
    RUST_BACKTRACE=1 cargo run --example end-to-end-local-net

test-sqlsync-reducer: wasm-sqlsync-reducer-guest
    cargo run --example host

dev-sqlsync-worker: wasm-sqlsync-worker
    cd lib/sqlsync-worker && pnpm dev

node_modules:
    cd lib/sqlsync-worker && pnpm i
    cd demo/frontend && pnpm i

# release targets below this point

upload-demo-reducer *FLAGS:
    #!/usr/bin/env bash
    set -euo pipefail
    just wasm-demo-reducer '{{FLAGS}}'
    cd demo/cloudflare-backend
    if [[ '{{FLAGS}}' = '--release' ]]; then
        REDUCER_PATH="../../target/wasm32-unknown-unknown/release/demo_reducer.wasm"
        echo "Uploading $REDUCER_PATH to cloudflare r2 bucket"
        pnpm exec wrangler r2 object put sqlsync-reducers/reducer.wasm --file ../../target/wasm32-unknown-unknown/release/demo_reducer.wasm
    else
        REDUCER_PATH="../../target/wasm32-unknown-unknown/debug/demo_reducer.wasm"
        echo "Uploading $REDUCER_PATH to cloudflare r2 dev bucket and local miniflare"
        pnpm exec wrangler r2 object put sqlsync-reducers-dev/reducer.wasm --file ../../target/wasm32-unknown-unknown/release/demo_reducer.wasm
        curl -X PUT -H "Content-Type: application/wasm" --data-binary @$REDUCER_PATH http://localhost:8787/reducer
    fi

package-sqlsync-worker +FLAGS='--release':
    just wasm-sqlsync-worker '{{FLAGS}}'
    cd lib/sqlsync-worker && pnpm build