-- search_links matches with `ilike '%q%'` across title, url and description.
-- The leading wildcard makes those predicates unindexable by btree, so every
-- search was a sequential scan over the whole table. Trigram GIN indexes do
-- serve them.
--
-- pg_trgm is a trusted extension as of PostgreSQL 13, so the role owning the
-- database can create it without being a superuser — which is what the NixOS
-- module's peer-authenticated role is.
create extension if not exists pg_trgm;

-- Every branch of the OR needs an index it can use: one unindexable branch
-- sends the whole predicate back to a sequential scan. The description branch
-- is written as `coalesce(description, '')`, so the index has to be on that
-- same expression rather than on the bare column.
create index links_title_trgm_idx on links using gin (title gin_trgm_ops);
create index links_url_trgm_idx on links using gin (url gin_trgm_ops);
create index links_description_trgm_idx on links using gin ((coalesce(description, '')) gin_trgm_ops);
