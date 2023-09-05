SQLSYNC_PROD_URL := "https://sqlsync.orbitinghail.workers.dev"

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

wasm-sqlsync +FLAGS='--dev':
    cd lib/sqlsync-worker/sqlsync-wasm && wasm-pack build --target web {{FLAGS}}

wasm-sqlsync-reducer-guest:
    cargo build --target wasm32-unknown-unknown --example guest

wasm-worker-test-reducer:
    cargo build --target wasm32-unknown-unknown --package test-reducer

wasm-demo-reducer *FLAGS:
    cargo build --target wasm32-unknown-unknown --package demo-reducer {{FLAGS}}

wasm-counter-reducer:
    cargo build --target wasm32-unknown-unknown --example counter-reducer

wasm-task-reducer:
    cargo build --target wasm32-unknown-unknown --example task-reducer

wasm-sqlsync-react-test-reducer:
    cargo build --target wasm32-unknown-unknown --package sqlsync-react-test-reducer

test-end-to-end-local: wasm-task-reducer
    RUST_BACKTRACE=1 cargo run --example end-to-end-local

test-end-to-end-local-net: wasm-counter-reducer
    RUST_BACKTRACE=1 cargo run --example end-to-end-local-net

test-sqlsync-reducer: wasm-sqlsync-reducer-guest
    cargo run --example host

dev-sqlsync-worker: wasm-sqlsync
    cd lib/sqlsync-worker && pnpm dev

node_modules:
    cd lib/sqlsync-worker && pnpm i
    cd demo/frontend && pnpm i

# mode should be either debug or release
# target should be either local or remote
upload-demo-reducer mode='release' target='local':
    #!/usr/bin/env bash
    set -euo pipefail
    cd demo/cloudflare-backend

    if [[ '{{mode}}' = 'release' ]]; then
        just wasm-demo-reducer '--release'
        REDUCER_PATH="../../target/wasm32-unknown-unknown/release/demo_reducer.wasm"
    else
        just wasm-demo-reducer
        REDUCER_PATH="../../target/wasm32-unknown-unknown/debug/demo_reducer.wasm"
    fi

    if [[ '{{target}}' = 'remote' ]]; then
        echo "Uploading $REDUCER_PATH to sqlsync prod"
        curl -X PUT --data-binary @$REDUCER_PATH {{SQLSYNC_PROD_URL}}/reducer
        echo
    else
        echo "Uploading $REDUCER_PATH to localhost:8787"
        curl -X PUT --data-binary @$REDUCER_PATH http://localhost:8787/reducer
        echo
    fi