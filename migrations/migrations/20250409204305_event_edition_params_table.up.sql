create table in_game_event_edition_params (
  id int auto_increment primary key,
  put_subtitle_on_newline boolean default false null,
  titles_pos char default 'L' null,
  lb_link_pos char default 'L' null,
  authors_pos char default 'R' null,
  titles_pos_x double null,
  titles_pos_y double null,
  lb_link_pos_x double null,
  lb_link_pos_y double null,
  authors_pos_x double null,
  authors_pos_y double null,
  constraint position_types check (
    in_game_event_edition_params.titles_pos is null
    or in_game_event_edition_params.titles_pos in ('L', 'R')
    and in_game_event_edition_params.lb_link_pos is null
    or in_game_event_edition_params.lb_link_pos in ('L', 'R')
    and in_game_event_edition_params.authors_pos is null
    or in_game_event_edition_params.authors_pos in ('L', 'R')
  )
);

alter table
  event_edition
add
  ingame_params_id int null;

alter table
  event_edition
add
  constraint event_edition___fk_ingame_params foreign key (ingame_params_id) references in_game_event_edition_params (id) on delete
set
  null;