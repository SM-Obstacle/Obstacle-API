# Records server

This repository contains the source code of the server that holds the Obstacle API. It manages
records saving, authentication, players and maps registering and all the stuff it goes with.

For now, it is splitted into different workspace members, but the main source code is located at the `game_api/` folder.

## Setup

The deployment of this project is made through Docker Compose.

Before starting the services, you must take a look at the `.docker/docker-compose.yml` file, especially the required secret files hidden by the `.gitignore` file. You must create them and store in them the required content (Database URL, Database root password, etc).

Then, go to the `.docker/` folder and run:
```sh
$ docker compose up -d
```

This will start all the services, with an Adminer at http://localhost:8080/ and the Obstacle API at http://localhost/
