FROM rust:1.70-buster AS base

ARG RECORDS_API_PORT

ENV USER=root

WORKDIR /code
RUN cargo init
COPY ./game_api/Cargo.toml /code/Cargo.toml
RUN cargo fetch
COPY ./game_api /code

FROM base AS development

EXPOSE ${RECORDS_API_PORT}

CMD ["cargo", "run", "--bin", "game-api", "--offline"]

FROM base AS dev-envs

EXPOSE 3000
RUN <<EOF
apt-get update
apt-get install -y --no-install-recommends git
EOF

RUN <<EOF
useradd -s /bin/bash -m vscode
groupadd docker
usermod -aG docker vscode
EOF

COPY --from=gloursdocker/docker / /
CMD ["cargo", "run", "--bin", "game-api", "--offline"]

FROM base AS builder

RUN cargo build --release --offline

FROM debian:bluster-slim

EXPOSE 3000

COPY --from=builder /code/target/release/game-api /game-api

CMD ["/game-api"]
