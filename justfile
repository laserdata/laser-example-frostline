set dotenv-load

up:
    ../laser-stack/scripts/up

down:
    ../laser-stack/scripts/down

up-local:
    scripts/stack-local start

down-local:
    scripts/stack-local stop

doctor:
    cargo run -p frostline-demo -- doctor

demo-once:
    cargo run -p frostline-demo -- finite

compare:
    cargo run -p frostline-demo -- compare

demo:
    cargo run -p frostline-demo -- live

codecs:
    cargo run -p frostline-demo -- codecs

inline:
    FROSTLINE_CATALOG=inline cargo run -p frostline-demo -- finite

scale records="10000000":
    FROSTLINE_TOTAL_RECORDS={{records}} FROSTLINE_RATE_PER_SECOND=1000000 FROSTLINE_CHECKPOINT_RECORDS=20000 FROSTLINE_POLL_RECORDS=1000 cargo run --release -p frostline-demo -- finite

soak rate="20000":
    FROSTLINE_RATE_PER_SECOND={{rate}} FROSTLINE_CHECKPOINT_RECORDS=20000 FROSTLINE_POLL_RECORDS=1000 cargo run --release -p frostline-demo -- live

report manifest:
    cargo run -p frostline-demo -- report --manifest {{manifest}}

cleanup manifest:
    cargo run -p frostline-demo -- cleanup --manifest {{manifest}}

bench profile:
    cargo build --release --workspace
    target/release/frostline-bench bench {{profile}}

selectivity:
    cargo build --release -p frostline-bench
    target/release/frostline-bench selectivity

# The local runtime under a memory cap, for the pressure trial: just up-pressure 1G, then just bench fleet_1m.
up-pressure limit="1G":
    systemd-run --user --scope --quiet -p MemoryMax={{limit}} -p MemorySwapMax=0 scripts/stack-local start

profile-cpu target seconds="30":
    cargo build --release -p frostline-bench
    target/release/frostline-bench profile-cpu {{target}} --seconds {{seconds}}

profile-memory target seconds="30":
    cargo build --release -p frostline-bench
    target/release/frostline-bench profile-memory {{target}} --seconds {{seconds}}

lint:
    cargo fmt --all -- --check
    cargo sort --workspace --check
    cargo machete --with-metadata
    scripts/max-lines 300
    cargo clippy --workspace --all-targets -- -D warnings
    cargo clippy --workspace --all-targets --all-features -- -D warnings

test:
    cargo test --workspace
    python3 -m unittest discover -s scripts/tests

doctest:
    cargo test --workspace --all-features --doc

test-it:
    cargo test -p frostline-shared --features integration
    cargo test -p frostline-producer --features integration
    cargo test -p frostline-consumers --features integration

e2e:
    cargo test -p frostline-demo --features e2e

ci: lint test doctest test-it e2e
