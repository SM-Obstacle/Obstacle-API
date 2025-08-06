CREATE VIEW global_event_records AS
with record as (select `r`.`record_id`        AS `record_id`,
                       `r`.`record_player_id` AS `record_player_id`,
                       `r`.`map_id`           AS `map_id`,
                       `r`.`time`             AS `time`,
                       `r`.`respawn_count`    AS `respawn_count`,
                       `r`.`record_date`      AS `record_date`,
                       `r`.`flags`            AS `flags`,
                       `r`.`try_count`        AS `try_count`,
                       `r`.`event_record_id`  AS `event_record_id`,
                       `eer`.`event_id`       AS `event_id`,
                       `eer`.`edition_id`     AS `edition_id`
                from (`records` `r` join `event_edition_records` `eer`
                      on (`eer`.`record_id` = `r`.`record_id`)))
select `r`.`record_id`        AS `record_id`,
       `r`.`record_player_id` AS `record_player_id`,
       `r`.`map_id`           AS `map_id`,
       `r`.`time`             AS `time`,
       `r`.`respawn_count`    AS `respawn_count`,
       `r`.`record_date`      AS `record_date`,
       `r`.`flags`            AS `flags`,
       `r`.`try_count`        AS `try_count`,
       `r`.`event_record_id`  AS `event_record_id`,
       `r`.`event_id`         AS `event_id`,
       `r`.`edition_id`       AS `edition_id`
from ((`record` `r` left join `record` `r3`
       on (`r3`.`map_id` = `r`.`map_id` and `r3`.`record_player_id` = `r`.`record_player_id` and
           `r3`.`time` < `r`.`time` and `r3`.`event_id` = `r`.`event_id` and
           `r3`.`edition_id` = `r`.`edition_id`)) left join `record` `r4`
      on (`r4`.`map_id` = `r`.`map_id` and `r4`.`record_player_id` = `r`.`record_player_id` and
          `r4`.`record_id` > `r`.`record_id` and `r4`.`time` = `r`.`time` and `r4`.`event_id` = `r`.`event_id` and
          `r4`.`edition_id` = `r`.`edition_id`))
where `r3`.`record_id` is null
  and `r4`.`record_id` is null
