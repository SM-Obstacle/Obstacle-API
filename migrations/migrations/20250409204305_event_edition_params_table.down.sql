alter table
  event_edition drop foreign key event_edition___fk_ingame_params;

alter table
  event_edition drop column ingame_params_id;

drop table in_game_event_edition_params;