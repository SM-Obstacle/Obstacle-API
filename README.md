# ShootMania Obstacle API

[![](https://img.shields.io/badge/records_lib-blue?style=flat&logo=rust&label=Doc&labelColor=gray)](http://obstacle-reborn.github.io/Records)

This repository contains the source code of the server that holds the ShootMania Obstacle API, with other services. It manages records saving, authentication, players and maps registering and all the stuff it goes with.

For now, it is splitted into different workspace members, but the main source code is located in the [`game_api/`](game_api/) and [`records_lib/`](records_lib/) packages.

The server handles a REST and GraphQL API. The REST API is mainly used by the Obstacle mode, and the GraphQL API is used by the website.

This project is not stable at all, and is subject to many changes, especially concerning its structure. For now, the `game_api/` folder contains mainly 2 subdirectories:
- [`graphql/`](game_api/src/graphql/): for the definitions of the GraphQL objects
- [`http/`](game_api/src/http/): for the definitions of the REST API routes

## Services

The Obstacle Records API uses 2 databases to manage the records. MySQL (more specifically MariaDB) contains all the persistent data transmitted by the API (information about a player, a record, etc), while Redis stores all the volatile data, that have to be accessed quickly, especially the leaderboards of the maps.

The API also includes a cache manager (the [`socc`](socc/) package).

## Documentation

You can find the crates documentation [here](http://obstacle-reborn.github.io/Records).