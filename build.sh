git pull
export RUSTFLAGS='-C target-cpu=native'
cargo build --color=always --workspace --all-targets --release
