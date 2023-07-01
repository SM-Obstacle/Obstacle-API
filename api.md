# Obstacle API

Note: This API is mainly used by the Obstacle Titlepack.
It has only access to GET and POST request methods. Thus, it may not respect the conventions of a REST API. In addition, it may change in the future, even drastically.

This file does not necessarily list all the endpoints correctly defined in the source code, as it may become cumbersome to keep synchronized both versions.

- [Obstacle API](#obstacle-api)
  - [GET `/overview`](#get-overview)
  - [GET `/graphql`](#get-graphql)
  - [POST `/graphql`](#post-graphql)
  - [POST `/player/update`](#post-playerupdate)
  - [POST `/player/finished`](#post-playerfinished)
  - [Authentication](#authentication)
    - [POST `/player/get_token`](#post-playerget_token)
    - [POST `/player/give_token`](#post-playergive_token)
    - [GET `/player/give_token`](#get-playergive_token)
  - [GET `/player/info`](#get-playerinfo)
  - [POST `/map/insert`](#post-mapinsert)
  - [GET `/event`](#get-event)
  - [GET `/event/:event_handle`](#get-eventevent_handle)
  - [GET `/event/:event_handle/:edition_id`](#get-eventevent_handleedition_id)

## GET `/overview`

Retrieves the leaderboard centered on a player in a map.

Query parameter     | Type   | Description
--------------------|--------|--------------------------------
`playerId`          | string | The login of the player (it has this name for compatibility reasons).
`mapId`             | string | The UID of the map.

An invalid `playerId` or `mapId` will result in a `400 Bad Request` error.

Example response:

```json
{
    "response": [
        {
            "rank": 1,
            "login": "wazas",
            "nickname": "...",
            "time": 39483
        },
        // ...
    ]
}
```

## GET `/graphql`

This is the entrypoint for the GraphQL playground.

## POST `/graphql`

This is the entrypoint for a GraphQL request. Use the playground to see the schema.

## POST `/player/update`

*This method requires [authentication](#authentication)*

Updates the information about a player, e.g. its nickname and its zone path.

Body:

```json
{
    "login": "Player's login",
    "nickname": "Player's nickname",
    "zone_path": "Player's zone path"
}
```

An invalid login will result in a `400 Bad Request` error.

Response: `200 OK` if everything went normal.

## POST `/player/finished`

*This method requires [authentication](#authentication)*

Registers the record of a player on a map.

Body:

```json
{
    "time": 34335, // in microseconds
    "respawn_count": 3,
    "login": "Player's login",
    "map_uid": "Map's UID",
    "flags": 0, // determines if ALT key was pressed or not for example
    "cps": [
        11445,
        11445,
        11445
    ]
}
```

An invalid `login` or `map_uid`, or the `cps` array length being greater than the cps number of the map,
or the sum of the `cps` times being different than the `time` provided, will result to a `400 Bad Request` error.

Example response:

```json
{
    "has_improved": true,
    "login": "Player's login",
    "old": 34312,
    "new": 34335
}
```

## Authentication

For all endpoints that require an authentication, you must provide an `Authorization` header,
with the Obstacle token of the player.

### POST `/player/get_token`

Requests a new token for a player. This request must be given with a `state` string, a randomly generated string,
that will be the same as the `<YOUR_STATE>` in this link when you open it to the player:

`https://prod.live.maniaplanet.com/login/oauth2/authorize?response_type=code&client_id=de1ce3ba8e&redirect_uri=https://obstacle.titlepack.io/give_token&state=<YOUR_STATE>&scope=basic`

Body:

```json
{
    "login": "Player's login",
    "state": "The randomly generated string",
    "redirect_uri": "https://obstacle.titlepack.io/give_token"
}
```

Note that this works even if the player doesn't yet exist in the database.

Response:

```json
{
    "token": "..."
}
```

### POST `/player/give_token`

This endpoint is used in the [GET `/player/give_token`] page to connect with
the [POST `/player/get_token`] endpoint, to give it the access token.
You usually don't need to use it directly.

### GET `/player/give_token`

This endpoint is the redirection page used by the ManiaPlanet OAuth2 system after
the user logged in.
You usually don't need to use it directly.

## GET `/player/info`

Retrieves the information about a player.

Body:

```json
{
    "login": "Player's login"
}
```

An invalid `login` will result in a `400 Bad Request` error.

Example response:

```json
{
    "id": 1056,
    "login": "ahmad3",
    "name": "ahmad",
    "join_date": "...",
    "zone_path": "World|Middle East|Lebanon",
    "role_name": "admin"
}
```

## POST `/map/insert`

Inserts a new map in the database, or does nothing if it already exists.
Note that if the author does not yet exist in the database, it is inserted on the fly.

Body:

```json
{
    "name": "Map's name",
    "map_uid": "Map's UID",
    "cps_number": 3,
    "author": {
        "login": "Author's login",
        "nickname": "Author's nickname",
        "zone_path": "Author's zone path"
    }
}
```

Response: `200 OK` if everything went normal.

## GET `/event`

Returns the all the list of the events, with the id of their last edition, or `null` if not yet registered.

Example response:

```json
[
    {
        "handle": "benchmark",
        "last_edition_id": 1
    },
    {
        "handle": "storm_runners",
        "last_edition_id": 2
    },
    {
        "handle": "atria_coop",
        "last_edition_id": null
    },
    // ...
]
```

## GET `/event/:event_handle`

Returns all the editions of an event from its handle, reverse-ordered by their ID.

Example response for `/event/storm_runners`:

```json
[
    {
        "id": 1,
        "start_date": "..."
    },
    {
        "id": 2,
        "start_date": "..."
    }
]
```

## GET `/event/:event_handle/:edition_id`

Returns the details of an edition of an event from its ID and the event handle.

Example response for `/event/storm_runners/2`:

```json
{
    "id": 2,
    "name": "Storm Runners #2",
    "start_date": "...",
    "banner_img_url": "...",
    "content": {
        "type": "Maps",
        "maps": [
            {
                "main_author": {
                    "login": "aurelamck",
                    "name": "$L[www.twitch.tv/aurelsm]$fffGrandp'Aurel$i$09F",
                    "zone_path": null
                },
                "other_authors": [
                    "Lyyras",
                    "Triss",
                    "rustydead",
                    "Lagaffe",
                    "artyoh"
                ],
                "name": "$o$nStorm Runners #2 - $F00The $A06Spirit $50CWorld",
                "map_uid": "LPOsdLqT5NKvefQT6y_vzZXsf23",
                "mx_id": 42418
            },
            // ...
        ]
    }
}
```

Note that the field `content.type` may be `"Maps"` or `"Categories"`.

For `"Maps"`, the next field will be `maps`, an array of the objects mentioned above.

For `"Categories"`, the next field will be `categories`, an array of these objets:

```json
{
    "handle": "The category's handle (for example: white)",
    "name": "The category's name (for example: White)",
    "banner_img_url": "...",
    "maps": [
        // ...same as above
    ]
}
```