# Obstacle API

## Get the player session token

Context: This request must be sent before any other request concerning players, to authenticate him. It gets a session token available for 12 hours, by providing the ManiaPlanet in-game auth token.
Path: `/player/get_token`
Method: `POST`
Header: `Content-Type: application/json`
Body (JSON):
```json
{
    "login": "string",
    // the ManiaPlanet auth token
    // see https://www.uaseco.org/maniascript/2019-10-10/struct_c_mania_app_title.html#a5ae0177b6fe5f741fcf0270f78168ea4
    "token": "string"
}
```
Response (XML):
```xml
<?xml version="1.0" encoding="UTF-8"?>
<GetTokenResponse>
    <!-- This token expires in 12h -->
    <token>Token</token>
</GetTokenResponse>
```

## Update

Context: Get the leaderboard of a map, centered on a specific player, by providing their information, to update the server.
Path: `/update`
Method: `POST`
Header: `Content-Type: application/json`
Body (JSON):
```json
{
    "secret": "token",
    "login": "string",
    "player": {
        "nickname": "string",
        "country": "country"
    },
    "map": {
        "name": "string",
        "maniaplanetMapId": "string",
        "author": {
            "login": "string",
            "nickname": "string",
            "zone_path": "string",
        }
    }
}
```
Response (XML):
```xml
<?xml version="1.0" encoding="UTF-8"?>
<response>
    <records>
        <rank>X</rank>
        <playerId>login</playerId>
        <nickname>nickname</nickname>
        <time>time</time>
    </records>
    <!-- ... -->
</response>
```
Check if the response code is 403 (forbidden), it means that the session token expired, so try to regenerate a session token with the `/player/get_token` request, then use it with the `/update` request.

## Player finishes a map

Context: Sent when a player finishes a map. This request must be sent before the inputs request with the same `state` string (see `/player/register_inputs` request).
Path: `/player/finished`
Method: `POST`
Header: `Content-Type: application/json`
Body (JSON):
```json
{
    // the player' session token sent by the Obstacle server
    // from the /player/get_token request.
    "secret": "token",
    // a random generated string
    "state": "string",
    "time": int,
    "respawnCount": int,
    "playerId": "login",
    "mapId": "mapUID",
    "flags" int,
    "cps": [
        {
            "cp_num": int,
            "time": int
        },
        // ...
    ]
}
```
This request will wait until the `/player/register_inputs` request has been sent with the same `state` string. If the `/player/register_inputs` request hasn’t been sent in the next 5 minutes, the `/player/finished` request will return a timeout error.
Response (XML):
```xml
<?xml version="1.0" encoding="UTF-8"?>
<response>
    <newBest>boolean</newBest>
    <login>record’s player login</login>
    <old>old time (same as new if first record on the map)</old>
    <new>new time (same as old if newBest is set at false)</new>
</response>
```
Check if the response code is 403 (forbidden), it means that the session token expired, so try to regenerate a session token with the `/player/get_token` request, then use it with the `/player/finished` request.

## Register player’s run inputs

Context: Sent when a player finishes a map. Must be sent after the `/player/finished` request.
Path: `/player/register_inputs`
Method: `POST`
Header: `Content-Type: multipart/form-data; boundary=--separation_string`
Body (multipart/form-data):
```
----separation_string
Content-Disposition: form-data; name="state"

state_string

--separation_string
Content-Disposition: form-data; name="inputs"
Content-Type: text/plain

content of the inputs file

--separation_string--
```
This request must be sent right after the `/player/finished` request with the same `state` string. If the `/player/finished` request hasn’t been sent, the `/player/register_inputs` request will return an error.
Response: OK

Setting note to a player (admin)

Context: Sets the admins’ note about a player.
Path: `/admin/set_note`
Method: `POST`
Header: `Content-Type: application/json`
Body (JSON):
```json
{
    // the admin' session token sent by the Obstacle server
    // from the /player/get_token request.
    "secret": "string",
    "admin_login": "string",
    "player_login": "string",
    "note": "string",
}
```
Response (XML):
```xml
<?xml version="1.0" encoding="UTF-8"?>
<SetNoteResponse>
    <admin_login>admin's login</admin_login>
    <login>login</login>
    <note>note</note>
</SetNoteResponse>
```
Check if the response code is 403 (forbidden), it means that the session token expired, so try to regenerate a session token with the `/player/get_token` request, then use it with the `/admin/set_note` request.

## Removing the note from a player (admin)

Context: Remove the admins’ note about a player from the database.
Path: `/admin/del_note`
Method: `POST`
Header: `Content-Type: application/json`
Body (JSON):
```json
{
    // the admin' session token sent by the Obstacle server
    // from the /player/get_token request.
    "secret": "string",
    "admin_login": "string",
    "player_login": "string"
}
```
Response (XML):
```xml
<?xml version="1.0" encoding="UTF-8"?>
<DelNoteResponse>
    <admin_login>admin's login</admin_login>
    <player_login>player's login</player_login>
</DelNoteResponse>
```
Check if the response code is 403 (forbidden), it means that the session token expired, so try to regenerate a session token with the `/player/get_token` request, then use it with the `/admin/del_note` request.

## Setting the role to a player (admin)

Context: Updates the role of a player (upgrade to Moderator, Admin, or downgrade to Moderator, Player).
Path: `/admin/set_role`
Method: `POST`
Header: `Content-Type: application/json`
Body (JSON):
```json
{
    // the admin' session token sent by the Obstacle server
    // from the /player/get_token request.
    "secret": "string",
    "admin_login": "string",
    "player_login": "string",
    // 0 => Player, 1 => Moderator, 2 => Admin
    "role": int
}
```
Response (XML):
```xml
<?xml version="1.0" encoding="UTF-8"?>
<SetRoleResponse>
    <admin_login>admin's login</admin_login>
    <player_login>player's login</player_login>
    <role>role name</role>
</SetRoleResponse>
```
Check if the response code is 403 (forbidden), it means that the session token expired, so try to regenerate a session token with the `/player/get_token` request, then use it with the `/admin/set_role` request.

## Get the banishments history for a player (admin)

Context: Get the banishments history of a player in the admin panel.
Path: `/admin/banishments`
Method: `GET`
Header: `Content-Type: application/json`
Body (JSON):
```json
{
    // the admin' session token sent by the Obstacle server
    // from the /player/get_token request.
    "secret": "string",
    "admin_login": "string",
    "player_login": "string"
}
```
Response (XML):
```xml
<?xml version="1.0" encoding="UTF-8"?>
<BanishmentsResponse>
    <admin_login>admin's login</admin_login>
    <player_login>player's login</player_login>
    <banishments>
        <banishment>
            <id>ban id</id>
            <date_ban>banishment date</date_ban>
            <duration>banishments duration (in seconds)</duration>
            <was_reprieved>boolean</was_reprieved>
            <reason>reason</reason>
            <banished_by>admin's login</banished_by>
        </banishment>
        ...
    </banishments>
</BanishmentsResponse>
```
Check if the response code is 403 (forbidden), it means that the session token expired, so try to regenerate a session token with the `/player/get_token` request, then use it with the `/admin/banishments` request.

## Ban a player (admin)

Context: Ban a player, by providing at least a reason and a duration.
Path: `/admin/ban`
Method: `POST`
Header: `Content-Type: application/json`
Body (JSON):
```json
{
    // the admin' session token sent by the Obstacle server
    // from the /player/get_token request.
    "secret": "string",
    "admin_login": "string",
    "player_login": "string",
    // in seconds
    "duration": int,
    "was_reprieved": boolean,
    "reason": "string"
}
```
Response (XML):
```xml
<?xml version="1.0" encoding="UTF-8"?>
<BanResponse>
    <admin_login>admin's login</admin_login>
    <player_login>player's login</player_login>
    <banishments>
        <banishment>
            <id>ban id</id>
            <date_ban>banishment date</date_ban>
            <duration>banishments duration (in seconds)</duration>
            <was_reprieved>boolean</was_reprieved>
            <reason>reason</reason>
            <banished_by>admin's login</banished_by>
        </banishment>
        ...
    </banishments>
</BanResponse>
```
Check if the response code is 403 (forbidden), it means that the session token expired, so try to regenerate a session token with the `/player/get_token` request, then use it with the `/admin/ban` request.

## Unban a player (admin)

Context: Unban a player, for any reason.
Path: `/admin/unban`
Method: `POST`
Header: `Content-Type: application/json`
Body (JSON):
```json
{
    // the admin' session token sent by the Obstacle server
    // from the /player/get_token request.
    "secret": "string",
    "admin_login": "string",
    "player_login": "string"
}
```
Response (XML):
```xml
<?xml version="1.0" encoding="UTF-8"?>
<BanResponse>
    <admin_login>admin's login</admin_login>
    <player_login>player's login</player_login>
    <banishments>
        <banishment>
            <id>ban id</id>
            <date_ban>banishment date</date_ban>
            <duration>banishments duration (in seconds)</duration>
            <was_reprieved>boolean</was_reprieved>
            <reason>reason</reason>
            <banished_by>admin's login</banished_by>
        </banishment>
        ...
    </banishments>
</BanResponse>
```
Check if the response code is 403 (forbidden), it means that the session token expired, so try to regenerate a session token with the `/player/get_token` request, then use it with the `/admin/ban` request.

## Get information about a player

Context: Get some information about a player (must be sent by him).
Path: `/player/info`
Method: `GET`
Header: `Content-Type: application/json`
Body (JSON):
```json
{
    "login": "string",
    // the player' session token sent by the Obstacle server
    // from the /player/get_token request.
    "secret": "string"
}
```
Response (XML):
```xml
<?xml version="1.0" encoding="UTF-8"?>
<InfoResponse>
    <id>id</id>
    <player_login>player's login</player_login>
    <name>nickname</name>
    <join_date>join date</join_date>
    <country>country</country>
    <play_time>play time (in seconds)</play_time>
    <role>role name</role>
</InfoResponse>
```
Check if the response code is 403 (forbidden), it means that the session token expired, so try to regenerate a session token with the `/player/get_token` request, then use it with the `/player/info` request.

## Get admins’ note about a player (admin)

Context: Get the admins’ note about a player.
Path: `/admin/player_note`
Method: `GET`
Header: `Content-Type: application/json`
Body (JSON):
```json
{
    "admins_login": "string",
    // the admin' session token sent by the Obstacle server
    // from the /player/get_token request.
    "secret": "string",
    "login": "string"
}
```
Response (XML):
```xml
<?xml version="1.0" encoding="UTF-8"?>
<AdminsNoteResponse>
    <player_login>player's login</player_login>
    <admins_note>note</admins_note>
</AdminsNoteResponse>
```
Check if the response code is 403 (forbidden), it means that the session token expired, so try to regenerate a session token with the `/player/get_token` request, then use it with the `/admin/player_note` request.

## Get the rating of a player on a map

Context: Get the ratings of a player on a map (must be sent by him).
Path: `/map/player_rating`
Method: `GET`
Header: `Content-Type: application/json`
Body (JSON):
```json
{
    // the player' session token sent by the Obstacle server
    // from the /player/get_token request.
    "secret": "string",
    "login": "string",
    "mapId": "string",
}
```
Response (XML):
```xml
<?xml version="1.0" encoding="UTF-8"?>
<PlayerRatingResponse>
    <player_login>player's login</player_login>
    <map_name>map's name</map_name>
    <author_login>login</author_login>
    <rating_date>rating date</rating_date>
    <ratings>
        <rating>
            <kind>kind</kind>
            <rating>rating (0 <= x <= 1)</rating>
        </rating>
        ...
    </ratings>
</PlayerRatingResponse>
```
Check if the response code is 403 (forbidden), it means that the session token expired, so try to regenerate a session token with the `/player/get_token` request, then use it with a new `/map/player_rating` request.

## Get all the ratings of a map (author/admin)

Context: Gets all the ratings (with the rating author, the kinds, and the note) of a map (must be done either by the map’s author, or an admin).
Path: `/map/ratings`
Method: `GET`
Header: `Content-Type: application/json`
Body (JSON):
```json
{
    // the session token of either the map's author or the admin,
    // sent by the Obstacle server from the /player/get_token request.
    "secret": "string",
    // the login of either the map's author or the admin.
    "login": "string",
    "mapId": "string",
}
```
Response (XML):
```xml
<?xml version="1.0" encoding="UTF-8"?>
<MapRatingsResponse>
    <map_name>map's name</map_name>
    <author_login>login</author_login>
    <players_ratings>
        <player_login>player's login</player_login>
        <rating_date>rating date</rating_date>
        <ratings>
            <rating>
                <kind>kind</kind>
                <rating>rating (0 <= x <= 1)</rating>
            </rating>
            ...
        </ratings>
    </players_ratings>
</MapRatingsResponse>
```
Check if the response code is 403 (forbidden), it means that the session token expired, so try to regenerate a session token with the `/player/get_token` request, then use it with a new `/map/player_rating` request.

## Get the rating of a map

Context: Get the average rating of a map.
Path: `/map/rating?mapId=map_uid`
Method: `GET`
Response (XML):
```xml
<?xml version="1.0" encoding="UTF-8"?>
<RatingResponse>
    <map_name>map's name</map_name>
    <author_login>login</author_login>
    <ratings>
        <rating>
            <kind>kind</kind>
            <rating>rating (0 <= x <= 1)</rating>
        </rating>
        ...
    </ratings>
</RatingResponse>
```

Update rating of a player on a map

Context: Updates the rating of a player on a map (must be sent by him).
Path: `/map/rate`
Method: `POST`
Header: `Content-Type: application/json`
Body (JSON):
```json
{
    // the player' session token sent by the Obstacle server
    // from the /player/get_token request.
    "secret": "string",
    "login": "string",
    "mapId": "string",
    "ratings": [
        {
            // 0 => route, 1 => deco, 2 => smoothness, 3 => difficulty
            "kind": int,
            // 0 <= x <= 1
            "rating": float
        },
        ...
    ]
}
```
Response (XML):
```xml
<?xml version="1.0" encoding="UTF-8"?>
<UpdateRateResponse>
    <player_login>player's login</player_login>
    <map_name>map's name</map_name>
    <author_login>login</author_login>
    <rating_date>rating date</rating_date>
    <ratings>
        <rating>
            <kind>kind</kind>
            <rating>rating (0 <= x <= 1)</rating>
        </rating>
        ...
    </ratings>
</UpdateRateResponse>
```
Check if the response code is 403 (forbidden), it means that the session token expired, so try to regenerate a session token with the `/player/get_token` request, then use it with the `/map/rate` request.

## Resets a map’s ratings (admin)

Context: Resets the ratings of a map, by deleting them.
Path: `/map/reset_rate`
Method: `POST`
Header: `Content-Type: application/json`
Body (JSON):
```json
{
    // the admin' session token sent by the Obstacle server
    // from the /player/get_token request.
    "secret": "string",
    "login": "string",
    "mapId": "string"
}
```
Response (XML):
```xml
<?xml version="1.0" encoding="UTF-8"?>
<ResetRatingsResponse>
    <player_login>player's login</player_login>
    <map_name>map's name</map_name>
    <author_login>login</author_login>
</ResetRatingsResponse>
```
Check if the response code is 403 (forbidden), it means that the session token expired, so try to regenerate a session token with the `/player/get_token` request, then use it with the `/map/rate` request.

## Get the medals of a player on a map

Context: Get the medals of a player on a map (must be sent by him).
Path: `/map/medals`
Method: `GET`
Header: `Content-Type: application/json`
Body (JSON):
```json
{
    // the player' session token sent by the Obstacle server
    // from the /player/get_token request.
    "secret": "string",
    "login": "string",
    "mapId": "string"
}
```
Response (XML):
```xml
<?xml version="1.0" encoding="UTF-8"?>
<GetMedalsResponse>
    <player_login>player's login</player_login>
    <map_name>map's name</map_name>
    <author_login>login</author_login>
    <prizes>
        <price>
            <price_date>date</price_date>
            <medal>medal name</medal>
        </price>
        ...
    </prizes>
</GetMedalsResponse>
```
Check if the response code is 403 (forbidden), it means that the session token expired, so try to regenerate a session token with the `/player/get_token` request, then use it with the `/map/medals` request.

## Add a medal to a player’s prizes on a map

Context: Add a new medal to the player’s prizes on a map (must be sent by him).
Path: `/map/add_medal`
Method: `POST`
Header: `Content-Type: application/json`
Body (JSON):
```json
{
    // the player' session token sent by the Obstacle server
    // from the /player/get_token request.
    "secret": "string",
    "login": "string",
    "mapId": "string",
    // 0 => bronze, 1 => silver, 2 => gold, 3 => champion
    "medal": int
}
```
Response (XML):
```xml
<?xml version="1.0" encoding="UTF-8"?>
<AddMedalResponse>
    <player_login>player's login</player_login>
    <map_name>map's name</map_name>
    <author_login>login</author_login>
    <prizes>
        <price>
            <price_date>date</price_date>
            <medal>medal name</medal>
        </price>
        ...
    </prizes>
</AddMedalResponse>
```
Check if the response code is 403 (forbidden), it means that the session token expired, so try to regenerate a session token with the `/player/get_token` request, then use it with the `/map/add_medal` request.

## Get all the events

Context: Get all the events, ordered by the date of their last edition.
Path: `/event`
Method: `GET`
Headers:
* `Accept: application/json`
Response (JSON):
```json
[
    {
        // acts like a login for a player
        "handle": "event_handle",
        // id of the last edition of this event or null if not yet existing
        "last_edition_id": 0,
    }
    // ...
]
```

## Get all the editions of an event

Context: Get all the editions of the given event, from its handle, ordered by their date.
Path: `/event/:event_handle`
Method: `GET`
Headers:
* `Accept: application/json`
Response (JSON):
```json
[
    {
        // the id of the edition
        "id": 0,
        "start_date": "date"
    }
    // ...
]
```

## Get the details of the edition of an event

Context: Get the details of an event edition, from the event handle and the edition id.
Path: `/event/:event_handle/:edition_id`
Method: `GET`
Headers:
* `Accept: application/json`
Response (JSON):
```json
{
    "id": 0,
    "name": "edition's name",
    "start_date": "date",
    "banner_img_url": "url",
    // if the edition has categories
    "content": {
        "categories": [
            {
                "handle": "white",
                "name": "White",
                "banner_img_url": "url",
                "maps": [
                    {
                        "game_id": "map's uid",
                        "dl_url": "mx url"
                    }
                    // ...
                ]
            }
            // ...
        ]
    },
    // else
    "content": {
        "maps": [
            {
                "game_id": "map's uid",
                "dl_url": "mx url"
            }
            // ...
        ]
    }
}
```