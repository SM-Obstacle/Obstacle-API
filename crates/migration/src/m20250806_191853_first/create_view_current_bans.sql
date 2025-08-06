CREATE VIEW `current_bans` AS
select
    `banishments`.`id` AS `id`,
    `banishments`.`date_ban` AS `date_ban`,
    `banishments`.`duration` AS `duration`,
    `banishments`.`was_reprieved` AS `was_reprieved`,
    `banishments`.`reason` AS `reason`,
    `banishments`.`player_id` AS `player_id`,
    `banishments`.`banished_by` AS `banished_by`,
    if(`banishments`.`duration` is null,
        NULL,
        (`banishments`.`date_ban` + interval `banishments`.`duration` second)
            - current_timestamp())
        AS `remaining_secs`
from `banishments`
where `banishments`.`date_ban` + interval `banishments`.`duration` second > current_timestamp()
    or `banishments`.`duration` is null
