# Records server

This repository contains the source code of the server that holds the Obstacle API. It manages
records saving, authentication, players and maps registering and all the stuff it goes with.

## Setup

### MariaDB server

MariaDB v10.11 or higher is required for the Obstacle database. You must edit the corresponding hardcoded URI in the `main.rs` file to the database, and set the same URI as the `DATABASE_URL` environment variable.

Then, you must run the SQL script joined with this project.

### Redis server

Redis v7.0 is also required to store cache for leaderboards and players' authentication tokens.

### Rust

Finally, you must install the latest Rust toolchain, usually with `rustup`.

## Run

Then, you may run the server as a service with `$ cargo run -r --bin game-api --no-default-features`