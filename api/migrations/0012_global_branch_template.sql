-- Global branch template. The per-repo `branch_template` becomes an optional
-- override of `settings.default_branch_template` (NULL = inherit the global
-- one), mirroring how `review_policy` already inherits `default_review_policy`.

ALTER TABLE repositories ALTER COLUMN branch_template DROP DEFAULT;
ALTER TABLE repositories ALTER COLUMN branch_template DROP NOT NULL;

-- Repos still holding the built-in default now inherit the global template. The
-- rendered branch is identical (the global default is the same string), and the
-- override field reads as "inherit" in the UI.
UPDATE repositories
    SET branch_template = NULL
    WHERE branch_template = 'seraphim/issue-{number}-{slug}';
