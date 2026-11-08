# Multi-stage build: compile the Rust binary, then ship a small runtime image.
FROM rust:1.99-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY README.md LICENSE ./
RUN cargo build --release --locked -p recall-mcp-cli --bin recall --bin recall-cli

FROM debian:bookworm-slim AS runtime
LABEL io.modelcontextprotocol.server.name="io.github.pranam-s/recall-mcp"
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home --uid 10001 recall \
    && mkdir -p /data \
    && chown recall:recall /data
COPY --from=build /src/target/release/recall /usr/local/bin/recall
COPY --from=build /src/target/release/recall-cli /usr/local/bin/recall-cli
COPY --from=build /src/LICENSE /usr/local/share/recall/LICENSE
USER recall
WORKDIR /data
ENV RECALL_MCP_DB=/data/recall.db
EXPOSE 8787
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8787/healthz || exit 1
ENTRYPOINT ["recall"]
CMD ["serve", "--bind", "0.0.0.0:8787", "--db", "/data/recall.db"]
