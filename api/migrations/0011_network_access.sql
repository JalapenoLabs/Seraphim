-- Network access whitelist: an outbound-connectivity policy for the agent's
-- workspace, modeled on Claude Code on the web's access levels. The selected
-- level (and, for `custom`, the allowed domains) is translated into the agent's
-- `~/.claude/settings.json` permissions during provisioning.

CREATE TYPE network_access_level AS ENUM ('none', 'trusted', 'full', 'custom');

-- Default to `full` so existing deployments keep their current, unrestricted
-- network access; operators opt into a stricter level deliberately.
ALTER TABLE settings
    ADD COLUMN network_access_level network_access_level NOT NULL DEFAULT 'full';

-- Custom allow-list, used only when network_access_level = 'custom'. One domain
-- per entry, e.g. ["api.example.com", "*.internal.example.com"].
ALTER TABLE settings
    ADD COLUMN network_access_domains JSONB NOT NULL DEFAULT '[]'::jsonb;

-- For `custom`: whether to also allow the built-in package-manager/registry
-- domains alongside the operator's list (the "Also include default list"
-- checkbox in the UI).
ALTER TABLE settings
    ADD COLUMN network_access_include_defaults BOOLEAN NOT NULL DEFAULT TRUE;
