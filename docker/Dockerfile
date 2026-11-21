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
HEALTHCHECK --interval=10m --timeout=40s \
	CMD curl -f "http://localhost:${RECORDS_API_PORT}/info" || bash -c 'kill -s 15 -1 && (sleep 10; kill -s 9 -1)'
COPY --from=builder /code/target/release/game-api /game-api
CMD ["/game-api"]

FROM runtime AS admin
COPY --from=builder /code/target/release/admin /admin
ENTRYPOINT ["/admin"]

FROM runtime AS socc
COPY --from=builder /code/target/release/socc /socc
CMD ["/socc"]

FROM runtime AS migrator
COPY --from=builder /code/target/release/migration /bin/migration
CMD ["/bin/migration"]
