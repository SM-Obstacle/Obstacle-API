-- Add up migration script here
alter table
    records
add
    modeversion varchar(11) null;