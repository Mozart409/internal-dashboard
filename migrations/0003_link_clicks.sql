-- How often a link has been opened from the UI, for the "most clicked" sort.
alter table links add column clicks bigint not null default 0;
