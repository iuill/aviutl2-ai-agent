# syntax=docker/dockerfile:1.7
FROM rust:1.97.1-bookworm@sha256:77fac8b98f9f46062bb680b6d25d5bcaabfc400143952ebc572e924bcbedc3fa AS dependencies

WORKDIR /src
COPY rust-toolchain.toml ./
RUN rustup target add x86_64-pc-windows-msvc \
 && cargo install cargo-xwin --version 0.23.0 --locked
COPY .cargo/config.toml .cargo/config.toml
COPY Cargo.toml Cargo.lock ./
COPY crates/cli/Cargo.toml crates/cli/Cargo.toml
COPY crates/plugin/Cargo.toml crates/plugin/Cargo.toml
COPY crates/protocol/Cargo.toml crates/protocol/Cargo.toml
COPY crates/mcp/Cargo.toml crates/mcp/Cargo.toml
RUN mkdir -p crates/cli/src crates/plugin/src crates/protocol/src crates/mcp/src \
 && printf 'fn main() {}\n' > crates/cli/src/main.rs \
 && printf 'fn main() {}\n' > crates/mcp/src/main.rs \
 && printf '' > crates/plugin/src/lib.rs \
 && printf '' > crates/protocol/src/lib.rs \
 && cargo test --locked --workspace --no-run \
 && cargo xwin build --locked --release --target x86_64-pc-windows-msvc -p aviutl2-ai-agent-plugin \
 && cargo xwin build --locked --release --target x86_64-pc-windows-msvc \
    -p aviutl2-ai-agent -p aviutl2-ai-agent-mcp

FROM dependencies AS build

ARG AVIUTL2_AI_AGENT_BUILD_COMMIT=""
ENV AVIUTL2_AI_AGENT_BUILD_COMMIT=${AVIUTL2_AI_AGENT_BUILD_COMMIT}

RUN rm -rf crates/cli/src crates/plugin/src crates/protocol/src crates/mcp/src
COPY crates ./crates
RUN find crates -type f -name '*.rs' -exec touch {} +
RUN cargo fmt --all --check
RUN cargo clippy --locked --workspace --all-targets -- -D warnings
RUN cargo test --locked --workspace
RUN cargo xwin build --locked --release --target x86_64-pc-windows-msvc -p aviutl2-ai-agent-plugin
RUN cargo xwin build --locked --release --target x86_64-pc-windows-msvc \
    -p aviutl2-ai-agent -p aviutl2-ai-agent-mcp
RUN mkdir /out \
 && cp target/x86_64-pc-windows-msvc/release/aviutl2_agent_plugin.dll /out/aviutl2-agent-plugin.aux2 \
 && cp target/x86_64-pc-windows-msvc/release/aviutl2-agent.exe /out/aviutl2-agent.exe \
 && cp target/x86_64-pc-windows-msvc/release/aviutl2-agent-mcp.exe /out/aviutl2-agent-mcp.exe \
 && cd /out \
 && sha256sum aviutl2-agent-plugin.aux2 aviutl2-agent.exe aviutl2-agent-mcp.exe > SHA256SUMS

FROM scratch AS export
COPY --from=build /out/ /
