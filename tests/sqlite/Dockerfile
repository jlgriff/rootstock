FROM rust:1-alpine AS build
RUN apk add --no-cache build-base
WORKDIR /source
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY mcp/Cargo.toml mcp/Cargo.toml
COPY mcp/src mcp/src
COPY tests/sqlite/Cargo.toml tests/sqlite/Cargo.toml
COPY tests/sqlite/src tests/sqlite/src
RUN cargo build --locked --release -p sqlite-fixture

FROM scratch
COPY --from=build /source/target/release/sqlite-fixture /sqlite-fixture
ENTRYPOINT ["/sqlite-fixture"]
CMD ["serve"]
HEALTHCHECK --interval=1s --timeout=2s --retries=20 CMD ["/sqlite-fixture", "health"]
