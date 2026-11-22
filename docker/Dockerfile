# One image per service, selected with --target:
#   docker build -f docker/Dockerfile --target backend -t obstacle-api/backend .

FROM rust:1.98.1-bookworm AS base

WORKDIR /code
COPY ./Cargo.toml ./Cargo.lock* ./
COPY ./crates ./crates
RUN cargo fetch

FROM base AS builder
ARG DB_BACKEND=mysql
RUN cargo build --release --offline --no-default-features -F $DB_BACKEND

FROM debian:bookworm-slim AS runtime
RUN apt-get update -y \
	&& apt-get install -y --no-install-recommends ca-certificates curl \
	&& rm -rf /var/lib/apt/lists/*

FROM runtime AS backend
# On failure, stop the API gracefully so the restart policy brings it back up.
HEALTHCHECK --interval=10m --timeout=40s \
	CMD curl -fsS --max-time 30 -o /dev/null "http://localhost:${RECORDS_API_PORT}/info" || { kill -s TERM 1; exit 1; }
COPY --from=builder /code/target/release/game-api /game-api
CMD ["/game-api"]

FROM runtime AS admin
COPY --from=builder /code/target/release/admin /admin
ENTRYPOINT ["/admin"]

FROM runtime AS socc
COPY --from=builder /code/target/release/socc /socc
CMD ["/socc"]

FROM runtime AS migrator
COPY ./docker/migrator-entrypoint.sh /entrypoint.sh
COPY --from=builder /code/target/release/migration /bin/migration
ENTRYPOINT ["/entrypoint.sh"]
CMD ["up"]
