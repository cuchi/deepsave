-- Tag registry: optional descriptions per tag, written by the user to teach the
-- AI what each tag means (injected into AI prompts). Tags on items/memory stay
-- free-form strings; this table only carries metadata, kept in sync by
-- rename/merge/delete (services/tags.rs).

CREATE TABLE IF NOT EXISTS tags (
  name        text PRIMARY KEY,
  description text NOT NULL DEFAULT '',
  created_at  timestamptz NOT NULL DEFAULT now()
);
