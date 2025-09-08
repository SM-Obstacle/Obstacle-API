CREATE VIEW global_records AS
with eer as (select `eer`.`record_id` AS `record_id`, `ee`.`non_original_maps` AS `non_original_maps`
             from (`event_edition_records` `eer` join `event_edition` `ee`
                   on (`ee`.`event_id` = `eer`.`event_id` and `ee`.`id` = `eer`.`edition_id`)))
select `r`.`record_id`        AS `record_id`,
       `r`.`record_player_id` AS `record_player_id`,
       `r`.`map_id`           AS `map_id`,
       `r`.`time`             AS `time`,
       `r`.`respawn_count`    AS `respawn_count`,
       `r`.`record_date`      AS `record_date`,
       `r`.`flags`            AS `flags`,
       `r`.`try_count`        AS `try_count`,
       `r`.`event_record_id`  AS `event_record_id`
from (((`records` `r` left join `records` `r3`
        on (`r3`.`map_id` = `r`.`map_id` and `r3`.`record_player_id` = `r`.`record_player_id` and
            `r3`.`time` < `r`.`time`)) left join `records` `r4`
       on (`r4`.`map_id` = `r`.`map_id` and `r4`.`record_player_id` = `r`.`record_player_id` and
           `r4`.`record_id` > `r`.`record_id` and `r4`.`time` = `r`.`time`)) left join `eer`
      on (`eer`.`record_id` = `r`.`record_id`))
where `r3`.`record_id` is null
  and `r4`.`record_id` is null
  and (`eer`.`record_id` is null or `eer`.`non_original_maps` <> 0)
