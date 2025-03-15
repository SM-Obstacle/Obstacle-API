# ShootMania Obstacle API

[![](https://img.shields.io/badge/records_lib-blue?style=flat&logo=rust&label=Doc&labelColor=gray)](https://sm-obstacle.github.io/Obstacle-API)

This repository contains the source code of the server that holds the ShootMania Obstacle API, with other services. It manages records saving, authentication, players and maps registering and all the stuff it goes with.

The API is splitted into different workspace members in the [`crates/`](crates/) folder.

The server handles a REST and GraphQL API. The REST API is mainly used by the Obstacle mode, and the GraphQL API is used by the website.

## Services

The Obstacle Records API uses 2 databases to manage the records. MySQL (more specifically MariaDB) contains all the persistent data transmitted by the API (information about a player, a record, etc), while Redis stores all the volatile data, that have to be accessed quickly, especially the leaderboards of the maps.

The API also includes a cache manager (the [`socc`](crates/socc/) package).

## Documentation

You can find the crates documentation [here](https://sm-obstacle.github.io/Obstacle-API).
