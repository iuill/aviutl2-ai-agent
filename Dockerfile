# syntax=docker/dockerfile:1.7
FROM rust:1.88.0-bookworm AS dependencies

RUN rustup target add x86_64-pc-windows-msvc \
 && cargo install cargo-xwin --version 0.19.2 --locked
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/cli/Cargo.toml crates/cli/Cargo.toml
COPY crates/plugin/Cargo.toml crates/plugin/Cargo.toml
COPY crates/protocol/Cargo.toml crates/protocol/Cargo.toml
RUN mkdir -p crates/cli/src crates/plugin/src crates/protocol/src \
 && printf 'fn main() {}\n' > crates/cli/src/main.rs \
 && printf '' > crates/plugin/src/lib.rs \
 && printf '' > crates/protocol/src/lib.rs \
 && cargo test --locked --workspace --no-run \
 && cargo xwin build --locked --release --target x86_64-pc-windows-msvc -p aviutl2-ai-agent-plugin \
 && cargo xwin build --locked --release --target x86_64-pc-windows-msvc -p aviutl2-ai-agent

FROM dependencies AS build

RUN rm -rf crates/cli/src crates/plugin/src crates/protocol/src
COPY crates ./crates
RUN touch crates/cli/src/*.rs crates/plugin/src/*.rs crates/protocol/src/*.rs
RUN cargo fmt --all --check
RUN cargo clippy --locked --workspace --all-targets -- -D warnings
RUN cargo test --locked --workspace
RUN cargo xwin build --locked --release --target x86_64-pc-windows-msvc -p aviutl2-ai-agent-plugin
RUN cargo xwin build --locked --release --target x86_64-pc-windows-msvc -p aviutl2-ai-agent
RUN mkdir /out \
 && cp target/x86_64-pc-windows-msvc/release/aviutl2_agent_plugin.dll /out/aviutl2-agent-plugin.aux2 \
 && cp target/x86_64-pc-windows-msvc/release/aviutl2-agent.exe /out/aviutl2-agent.exe \
 && cd /out \
 && sha256sum aviutl2-agent-plugin.aux2 aviutl2-agent.exe > SHA256SUMS

FROM scratch AS export
COPY --from=build /out/ /
