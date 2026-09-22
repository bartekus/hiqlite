set shell := ["bash", "-uc"]

export TAG := `cat hiqlite/Cargo.toml | grep '^version =' | cut -d " " -f3 | xargs`
export MSRV := `cat hiqlite/Cargo.toml | grep '^rust-version =' | cut -d " " -f3 | xargs`
export USER := `echo "$(id -u):$(id -g)"`

[private]
default:
    @just -l

# Creates a new Root + Intermediate CA for development and testing TLS certificates
create-root-ca:
    # Password for both root and intermediate dev CA is always: 123SuperMegaSafe

    mkdir -p tls/ca
    chmod 0766 tls/ca

    # Root CA
    docker run --rm -it -v ./tls/ca:/ca -u $USER \
          ghcr.io/sebadob/nioca \
          x509 \
          --stage root \
          --clean

    # Intermediate CA
    docker run --rm -it -v ./tls/ca:/ca -u $USER \
          ghcr.io/sebadob/nioca \
          x509 \
          --stage intermediate

    cp tls/ca/x509/intermediate/ca-chain.pem tls/ca-chain.pem

# Create a new End Entity TLS certificate for development and testing

# Intermediate CA DEV password: 123SuperMegaSafe
create-end-entity-tls:
    # create the new certificate
    docker run --rm -it -v ./tls/ca:/ca -u $USER \
          ghcr.io/sebadob/nioca \
          x509 \
          --cn 'localhost' \
          --alt-name-dns 'localhost' \
          --alt-name-dns 'hiqlite.local' \
          --alt-name-ip '127.0.0.1' \
          --usages-ext server-auth \
          --usages-ext client-auth \
          --o 'Hiqlite DEV Certificate' \
          --stage end-entity

    # copy it in the correct place
    cp tls/ca/x509/end_entity/$(cat tls/ca/x509/end_entity/serial)/cert-chain.pem tls/cert-chain.pem
    cp tls/ca/x509/end_entity/$(cat tls/ca/x509/end_entity/serial)/key.pem tls/key.pem

# prints out the currently set version
version:
    #!/usr/bin/env bash
    echo "v$TAG"

# cleanup the data dir
cleanup:
    rm -rf data/*

# clippy lint + check with minimal versions from nightly
check:
    #!/usr/bin/env bash
    set -euxo pipefail
    clear
    cargo update
    cargo clippy -- -D warnings
    cargo minimal-versions check -p hiqlite-patched --features server
    cargo minimal-versions check -p hiqlite-patched --no-default-features --features external-state-machine
    cargo minimal-versions check -p hiqlite-wal-patched

    # update at the end again for following clippy and testing
    cargo update

# checks all combinations of features with clippy
clippy:
    #!/usr/bin/env bash
    set -euxo pipefail
    clear

    cargo clippy --no-default-features -- -D warnings

    cargo clippy --no-default-features --features sqlite,cast_ints -- -D warnings
    cargo clippy --no-default-features --features sqlite,cast_ints_unchecked -- -D warnings
    # auto-heal should only apply to sqlite
    cargo clippy --no-default-features --features auto-heal -- -D warnings
    cargo clippy --no-default-features --features sqlite,auto-heal -- -D warnings
    # backup / s3 should only apply to sqlite
    cargo clippy --no-default-features --features backup -- -D warnings
    cargo clippy --no-default-features --features sqlite,backup -- -D warnings
    cargo clippy --no-default-features --features sqlite,auto-heal,backup -- -D warnings

    cargo clippy --no-default-features --features cache -- -D warnings
    cargo clippy --no-default-features --features in-memory-snapshots -- -D warnings
    cargo clippy --no-default-features --features counters -- -D warnings
    cargo clippy --no-default-features --features dlock -- -D warnings
    cargo clippy --no-default-features --features listen_notify_local -- -D warnings
    cargo clippy --no-default-features --features listen_notify -- -D warnings
    cargo clippy --no-default-features --features sqlite,cache,webpki-roots -- -D warnings

    cargo clippy --no-default-features --features dashboard -- -D warnings
    cargo clippy --no-default-features --features shutdown-handle -- -D warnings

    # external-state-machine must build standalone without pulling in the
    # internal `__cluster` boundary, and must not break any regular combination
    cargo clippy --no-default-features --features external-state-machine -- -D warnings
    cargo clippy --no-default-features --features sqlite,external-state-machine -- -D warnings
    cargo clippy --no-default-features --features full,external-state-machine -- -D warnings
    cargo clippy --features external-state-machine -- -D warnings

clippy-examples:
    #!/usr/bin/env bash
    set -euxo pipefail
    clear

    cd examples
    for example in */; do
      cd $example
      cargo clippy
      cd ..
    done
    cd ..

# build and open the docs
docs:
    cargo +nightly doc --all-features --no-deps --open

# runs the full set of tests
test test="":
    #!/usr/bin/env bash
    set -euxo pipefail
    clear
    cargo test --features cache,counters,dlock,listen_notify,macros,toml,external-state-machine {{ test }}

# runs the full set of tests excluding backup to S3 tests
test-no-s3:
    #!/usr/bin/env bash
    set -euxo pipefail
    clear
    TEST_SKIP_S3_RESTORE="true" cargo test --features cache,counters,dlock,listen_notify,macros,toml,external-state-machine

# builds the code
build ty="server":
    #!/usr/bin/env bash
    set -euxo pipefail

    if [[ {{ ty }} == "server" ]]; then
          cargo build
    elif [[ {{ ty }} == "ui" ]]; then
      rm -rf hiqlite/static
      cd dashboard
      rm -rf build
      npm run build
      git add ../hiqlite/static
    fi

# builds a container image
build-image name="ghcr.io/sebadob/hiqlite":
    #!/usr/bin/env bash
    set -euxo pipefail

    rm -rf hiqlite/static
    cd dashboard
    npm run build
    cd ..
    git add hiqlite/static

    #cargo build --features server --release
    #mkdir -p out
    #cp target/release/hiqlite out/

    docker build -t {{ name }}:{{ TAG }} .
    docker push {{ name }}:{{ TAG }}
    docker tag {{ name }}:{{ TAG }} {{ name }}:latest
    docker push {{ name }}:latest

# builds the code in --release mode
build-release:
    #!/usr/bin/env bash
    set -euxo pipefail
    cargo build --release

run ty="server" node_id="1":
    #!/usr/bin/env bash
    set -euxo pipefail
    clear

    if [[ {{ ty }} == "server" ]]; then
      HQL_DATA_DIR=data/server_{{ node_id }} cargo run --features server,backup -- serve -c hiqlite.toml --node-id {{ node_id }}
    elif [[ {{ ty }} == "ui" ]]; then
      cd dashboard
      npm run dev -- --host=0.0.0.0
    fi

test-migrate:
    #!/usr/bin/env bash
    set -euxo pipefail
    clear
    cp -r data/logs_bkp/* data/server_1/logs/
    HQL_DATA_DIR=data/server_1 cargo run --features server -- serve -c hiqlite.env --node-id 1

# verifies the MSRV
msrv-verify:
    #!/usr/bin/env bash
    set -euxo pipefail
    cd hiqlite
    cargo msrv verify
    cd ..

    cd hiqlite-derive
    cargo msrv verify
    cd ..

    cd hiqlite-wal
    cargo msrv verify

# find's the new MSRV, if it needs a bump
msrv-find:
    cargo msrv find --min {{ MSRV }} --all-features

# verify thats everything is good
verify:
    # we don't want to rebuild the UI each time because it's checked into git
    #just build ui
    just check
    just clippy
    just clippy-examples
    just test
    just msrv-verify

# makes sure everything is fine
verify-is-clean: verify
    #!/usr/bin/env bash
    set -euxo pipefail

    # make sure everything has been committed
    git diff --exit-code

    echo all good

# sets a new git tag and pushes it
release:
    #!/usr/bin/env bash
    set -euxo pipefail

    # make sure git is clean
    git diff --quiet || exit 1

    git tag "v$TAG"
    git push origin "v$TAG"

    just build-image

# Packaging check for one crate, without touching the registry.
#
# `--no-verify` is deliberately absent everywhere in this file: it skips the build of the
# packaged tree, which is the only thing that catches a manifest that resolves in the workspace
# and not from the registry.
package-check:
    #!/usr/bin/env bash
    set -euxo pipefail
    cargo package -p hiqlite-wal-patched --allow-dirty
    cargo package -p hiqlite-derive-patched --allow-dirty
    # `hiqlite-patched` cannot be packaged until its two dependencies are on the registry: its
    # published manifest resolves them by version. `--no-verify` would hide that rather than
    # answer it, so this stops here and `publish-core` is what proves it.
    echo "wal and derive package cleanly; core is verified at publication time"

# Publication order is the dependency order: wal and derive first, then core, which depends on
# both by version. Each step waits for the registry to serve what the next one needs.
publish-wal:
    #!/usr/bin/env bash
    set -euxo pipefail
    cargo publish -p hiqlite-wal-patched

publish-derive:
    #!/usr/bin/env bash
    set -euxo pipefail
    cargo publish -p hiqlite-derive-patched

publish-core:
    #!/usr/bin/env bash
    set -euxo pipefail
    cargo publish -p hiqlite-patched

# does a `cargo update` + `npm update` for the UI
update:
    #!/usr/bin/env bash

    # We need at least nightly-2026-06-21 for the min release age feature
    # from .cargo/config.toml
    MIN_DATE="2026-06-21"
    NIGHTLY_DATE=$(rustc +nightly --version | grep -oE '[0-9]{4}-[0-9]{2}-[0-9]{2}' | head -n 1)
    if [[ -z "$NIGHTLY_DATE" || "$NIGHTLY_DATE" < "$MIN_DATE" ]]; then
        echo "Error: The nightly toolchain must be at least $MIN_DATE. (Found: ${NIGHTLY_DATE:-unknown})"
        exit 1
    fi
    cargo +nightly update

    cd dashboard
    # min release is set via `dashboard/.npmrc`
    npm update

# Governance tool pin. The revision is newer than the v0.20.0 tag while still
# reporting 0.20.0, so the revision is the reproducibility boundary.
spec-spine-rev := "aa559f5dcaa59bd9f27b0622b51ae5b57dc2185f"

# Install the exact spec-spine revision used by CI.
spine-install:
    cargo install spec-spine-cli --git https://github.com/statecrafting/spec-spine --rev {{ spec-spine-rev }} --locked

# Regenerate the committed registry and codebase-index shards after a trusted edit.
spine-regenerate:
    spec-spine compile
    spec-spine index

# Read-only corpus, freshness, lint, and bounded coverage checks.
spine-check:
    spec-spine check --fail-on-unresolved --fail-on-warn
    spec-spine lint --fail-on-warn
    spec-spine index coverage

# Compare a branch to its actual pull-request base. The default is this fork's
# integration branch, which is a convenience for a local run and is correct only
# for a branch that merges there. CI does not use it: the workflow passes the
# pull request's real base SHA.
spine-couple base="origin/spec-spine" head="HEAD":
    spec-spine couple --base "{{ base }}" --head "{{ head }}"

# Run one trusted specification's executable acceptance block locally.
spine-verify spec:
    spec-spine verify "{{ spec }}"
