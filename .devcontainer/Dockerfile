FROM mcr.microsoft.com/devcontainers/rust:1-bookworm@sha256:f5b84c5f3411a4366dc562ea9fdbb82cc90e5fd7332c8d523e342bbd8ad763d8

ARG RUST_VERSION=1.88.0
ARG CARGO_XWIN_VERSION=0.19.2
ARG OPENAI_CODEX_VERSION=0.145.0

USER root

RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    nodejs \
    npm \
    ripgrep \
 && rustup toolchain install "${RUST_VERSION}" --profile minimal --component clippy,rustfmt \
 && rustup default "${RUST_VERSION}" \
 && rustup target add --toolchain "${RUST_VERSION}" x86_64-pc-windows-msvc \
 && cargo install cargo-xwin --version "${CARGO_XWIN_VERSION}" --locked \
 && npm install --global "@openai/codex@${OPENAI_CODEX_VERSION}" \
 && mv /usr/local/bin/codex /usr/local/bin/codex-bin \
 && apt-get clean \
 && rm -rf /var/lib/apt/lists/*

COPY .devcontainer/bin/codex /usr/local/bin/codex
COPY .devcontainer/bin/docker /usr/local/bin/docker
COPY .devcontainer/docker-daemon.json /etc/docker/daemon.json

RUN chmod 0755 /usr/local/bin/codex /usr/local/bin/docker \
 && mkdir -p /home/vscode/.codex /home/vscode/.config/gh \
 && chown -R vscode:vscode \
    /home/vscode/.codex \
    /home/vscode/.config/gh \
    /usr/local/cargo

USER vscode
