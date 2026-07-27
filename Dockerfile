# syntax=docker/dockerfile:1.7
FROM rust:1.88.0-bookworm AS build

RUN rustup target add x86_64-pc-windows-msvc \
 && cargo install cargo-xwin --version 0.19.2 --locked
WORKDIR /src
COPY . .
RUN cargo fmt --all --check
RUN cargo clippy --locked --workspace --all-targets -- -D warnings
RUN cargo test --locked --workspace
RUN cargo xwin build --locked --release --target x86_64-pc-windows-msvc -p aviutl2-agent-plugin
RUN cargo xwin build --locked --release --target x86_64-pc-windows-msvc -p aviutl2-agent
RUN mkdir /out \
 && cp target/x86_64-pc-windows-msvc/release/aviutl2_agent_plugin.dll /out/aviutl2-agent-plugin.aux2 \
 && cp target/x86_64-pc-windows-msvc/release/aviutl2-agent.exe /out/aviutl2-agent.exe \
 && cd /out \
 && sha256sum aviutl2-agent-plugin.aux2 aviutl2-agent.exe > SHA256SUMS

FROM scratch AS export
COPY --from=build /out/ /
