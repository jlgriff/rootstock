FROM rust:1.98-alpine AS build
RUN apk add --no-cache musl-dev
WORKDIR /source
COPY Cargo.toml Cargo.lock ./
COPY src ./src
ARG TARGETARCH
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,id=rootstock-leptos-${TARGETARCH},target=/source/target \
    cargo build --locked --release && cp target/release/leptos-placeholder /site

FROM scratch
COPY --from=build /site /site
USER 1000:1000
EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 CMD ["/site", "--healthcheck"]
ENTRYPOINT ["/site"]
