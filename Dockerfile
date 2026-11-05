FROM rust:1.98-alpine AS build
RUN apk add --no-cache musl-dev
WORKDIR /source
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY mcp ./mcp
COPY tests/sqlite ./tests/sqlite
ARG TARGETARCH
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,id=rootstock-dashboard-${TARGETARCH},target=/source/target \
    RUSTFLAGS='-C strip=symbols' cargo build --locked --release -p rootstock \
    && cp target/release/rootstock /rootstock

FROM docker:29.9.3-cli@sha256:2357639ad1ea7d0194134bf3292622fd4e562483966ed674e5d7c945b5f3b28d AS docker

FROM alpine:3.24.3@sha256:be2656061b536c1883c4034790e21d0a09c5aaaf51968b89368a3c0371ea2c9a
COPY --from=docker /usr/local/bin/docker /usr/local/bin/docker
COPY --from=build /rootstock /usr/local/bin/rootstock
ENV HOME=/tmp DOCKER_CONFIG=/tmp/docker
USER 1000:1000
EXPOSE 8787
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 CMD wget -q -O /dev/null http://127.0.0.1:8787/favicon.svg || exit 1
ENTRYPOINT ["/usr/local/bin/rootstock"]
CMD ["observe"]
