create table links (
    id          uuid primary key default gen_random_uuid(),
    url         text not null,
    title       text not null,
    description text,
    tags        text[] not null default '{}',
    created_at  timestamptz not null default now(),
    updated_at  timestamptz not null default now()
);

create index links_created_at_idx on links (created_at desc);
create index links_tags_idx on links using gin (tags);
