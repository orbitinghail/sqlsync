SQLSYNC_PROD_URL := "https://sqlsync.orbitinghail.workers.dev"

default:
    @just --choose

unit-test:
    cargo test

build: build-wasm
    cargo build -p sqlsync

build-wasm:
    just run-with-prefix 'wasm-'

run-with-prefix prefix:
    #!/usr/bin/env bash
    set -euo pipefail
    all_tasks=$(just --summary)

    for task in $all_tasks; do
        if [[ $task == {{prefix}}* ]]; then
            just $task
        fi
    done

wasm-sqlsync +FLAGS='--dev':
    cd lib/sqlsync-worker/sqlsync-wasm && wasm-pack build --target web {{FLAGS}}

wasm-sqlsync-reducer-guest:
    cargo build --target wasm32-unknown-unknown --example guest

wasm-demo-reducer *FLAGS:
    cargo build --target wasm32-unknown-unknown --package demo-reducer {{FLAGS}}

wasm-counter-reducer:
    cargo build --target wasm32-unknown-unknown --example counter-reducer

wasm-task-reducer:
    cargo build --target wasm32-unknown-unknown --example task-reducer

wasm-examples-reducer-guestbook:
    cargo build --target wasm32-unknown-unknown --package reducer-guestbook --release

example-guestbook-react: wasm-examples-reducer-guestbook
    cd examples/guestbook-react && pnpm dev

example-guestbook-solid-js: wasm-examples-reducer-guestbook
    cd examples/guestbook-solid-js && pnpm dev

test-end-to-end-local rng_seed="": wasm-task-reducer
    RUST_BACKTRACE=1 cargo run --example end-to-end-local {{rng_seed}}

test-end-to-end-local-net rng_seed="": wasm-counter-reducer
    RUST_BACKTRACE=1 cargo run --example end-to-end-local-net {{rng_seed}}

test-sqlsync-reducer: wasm-sqlsync-reducer-guest
    cargo run --example host

node_modules:
    pnpm i

package-sqlsync-react:
    cd lib/sqlsync-react && pnpm build

package-sqlsync-solid-js:
    cd lib/sqlsync-solid-js && pnpm build

package-sqlsync-worker target='release':
    #!/usr/bin/env bash
    if [[ '{{target}}' = 'release' ]]; then
        cd lib/sqlsync-worker && pnpm build-release
    else
        cd lib/sqlsync-worker && pnpm build
    fi

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

publish-sqlsync-worker: (package-sqlsync-worker "release")
    cd lib/sqlsync-worker && pnpm publish --access public

publish-sqlsync-react: package-sqlsync-react
    cd lib/sqlsync-react && pnpm publish --access public

publish-sqlsync-reducer:
    cd lib/sqlsync-reducer && cargo publish

publish-demo-backend:
    cd demo/cloudflare-backend && pnpm wrangler-deploy

publish-demo-frontend: (package-sqlsync-worker "release") package-sqlsync-react
    cd demo/frontend && pnpm release
