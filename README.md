# Records server

[![](https://img.shields.io/badge/records_lib-blue?style=flat&logo=rust&label=Doc&labelColor=gray)](http://obstacle-reborn.github.io/Records)

This repository contains the source code of the server that holds the Obstacle API. It manages
records saving, authentication, players and maps registering and all the stuff it goes with.

For now, it is splitted into different workspace members, but the main source code is located at the `game_api/` folder.

The API handles a REST and GraphQL API. The REST API is mainly used by the Obstacle mode, and the GraphQL API
is used by the website.

This project is not stable at all, and is subject to many changes, especially concerning its structure.
For now, the `game_api/` folder contains mainly 2 subdirectories:
- `graphql/`: for the definitions of the GraphQL objects
- `http/`: for the definitions of the REST API routes

## Services

The Obstacle Records API uses 2 databases to manage the records.
MySQL (more specifically MariaDB) contains all the persistent data transmitted by the API (information about a player, a record, etc), while Redis stores all the volatile data, that have to be accessed quickly, especially the leaderboards of the maps.
