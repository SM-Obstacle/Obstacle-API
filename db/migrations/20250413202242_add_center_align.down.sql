alter table
  in_game_event_edition_params drop constraint position_types;

alter table
  in_game_event_edition_params
add
  constraint position_types check (
    `titles_align` is null
    or `titles_align` in ('L', 'R')
    and `lb_link_align` is null
    or `lb_link_align` in ('L', 'R')
    and `authors_align` is null
    or `authors_align` in ('L', 'R')
  );