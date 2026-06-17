# The agent's workspace sandbox (issue #77).
#
# Based on Microsoft's universal devcontainer image: Python, Node (via nvm), Go,
# Java, .NET, PHP, Ruby, plus version managers, out of the box. On top we add the
# tools the agents need that it doesn't ship: Rust/Cargo, the Claude Code CLI, the
# GitHub CLI, a Docker client, Earthly, actionlint, the org cloud CLIs (jira,
# doctl, gcloud, wrangler, bunny), and Playwright's Chromium for Plunder's e2e
# suite. Work runs as the image's non-root `codespace` user; the API drives it
# via `docker exec`, the container itself just idles.

FROM mcr.microsoft.com/devcontainers/universal:latest

# Static Docker client version pulled below (client only; talks to host socket).
ARG DOCKER_VERSION=27.3.1
USER root

# --- Base CLIs we rely on (most already present; ensure the rest) ------------
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates curl gnupg git jq sudo less ripgrep openssh-client \
    && rm -rf /var/lib/apt/lists/*

# --- PostgreSQL 17 (client + server) for local DB validation -----------------
# The base image's distro ships an older Postgres than the PG17 the API, CI, and
# production all run, so a hand-installed cluster would drift onto PG16. Install
# the matching major from the official PostgreSQL APT repository (PGDG) so the
# agent can stand up a throwaway PG17 (see the `pg-ephemeral` helper below) and
# verify migrations and integration tests locally before pushing.
RUN install -d -m 0755 /etc/apt/keyrings \
    && curl -fsSL https://www.postgresql.org/media/keys/ACCC4CF8.asc \
        -o /etc/apt/keyrings/pgdg.asc \
    && echo "deb [signed-by=/etc/apt/keyrings/pgdg.asc] https://apt.postgresql.org/pub/repos/apt $(. /etc/os-release && echo "$VERSION_CODENAME")-pgdg main" \
        > /etc/apt/sources.list.d/pgdg.list \
    && apt-get update \
    && apt-get install -y --no-install-recommends postgresql-17 postgresql-client-17 \
    && rm -rf /var/lib/apt/lists/*

# --- GitHub CLI (release tarball; no dependency on an apt source) ------------
RUN tag="$(curl -fsSL -o /dev/null -w '%{url_effective}' https://github.com/cli/cli/releases/latest | sed -E 's#.*/tag/##')" \
    && ver="${tag#v}" \
    && curl -fsSL "https://github.com/cli/cli/releases/download/${tag}/gh_${ver}_linux_amd64.tar.gz" -o /tmp/gh.tgz \
    && tar -xzf /tmp/gh.tgz -C /tmp \
    && install -m 0755 "/tmp/gh_${ver}_linux_amd64/bin/gh" /usr/local/bin/gh \
    && rm -rf /tmp/gh*

# --- Docker client (static binary; talks to the mounted host socket) ---------
RUN curl -fsSL "https://download.docker.com/linux/static/stable/x86_64/docker-${DOCKER_VERSION}.tgz" \
        | tar xz --strip-components=1 -C /usr/local/bin docker/docker

# --- Earthly -----------------------------------------------------------------
RUN curl -fsSL https://github.com/earthly/earthly/releases/latest/download/earthly-linux-amd64 \
        -o /usr/local/bin/earthly \
    && chmod +x /usr/local/bin/earthly

# --- actionlint (GitHub Actions workflow linter) -----------------------------
# Lets the agent validate `.github/workflows/*.yml` locally (bad `${{ }}`
# expressions, unknown contexts, shell mistakes) before pushing, instead of
# fetching the release binary ad hoc each time (issues #220, #252). Pinned,
# prebuilt release tarball (the binary sits at the archive root), matching the
# gh/doctl pattern above; no Go toolchain needed.
ARG ACTIONLINT_VERSION=1.7.12
RUN curl -fsSL "https://github.com/rhysd/actionlint/releases/download/v${ACTIONLINT_VERSION}/actionlint_${ACTIONLINT_VERSION}_linux_amd64.tar.gz" -o /tmp/actionlint.tgz \
    && tar -xzf /tmp/actionlint.tgz -C /tmp actionlint \
    && install -m 0755 /tmp/actionlint /usr/local/bin/actionlint \
    && rm -rf /tmp/actionlint*

# Verification gate (issue #252): #220 installed actionlint but it was reported
# unresolvable on PATH in the running container, and the ticket closed without
# this check. Fail the BUILD here unless `actionlint --version` resolves both for
# root (the default `docker exec` user) and for the `codespace` agent user in a
# login shell, the exact context of the acceptance check
# (`docker exec seraphim-workspace bash -lc 'actionlint --version'`). A binary
# that lands off PATH can no longer ship silently.
RUN actionlint --version \
    && runuser -l codespace -c 'actionlint --version'

# --- Rust / Cargo (not in the universal image) -------------------------------
# Installed system-wide so every user/shell finds it on PATH; the registry/cache
# dirs are world-writable so the non-root agent can build and fetch crates.
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --no-modify-path --profile minimal --default-toolchain stable \
    && rustup component add rustfmt clippy \
    # Pre-install the toolchain api/rust-toolchain.toml pins, with fmt + clippy,
    # so `cargo +1.88 fmt --check` / `clippy` / `test` work on a fresh workspace
    # with no first-run download or "component not installed" stall.
    && rustup toolchain install 1.88 --profile minimal \
    && rustup component add --toolchain 1.88 rustfmt clippy \
    && chmod -R a+w "$RUSTUP_HOME" "$CARGO_HOME"

# --- Org cloud CLIs (jira, doctl, gcloud) ------------------------------------
RUN tag="$(curl -fsSL -o /dev/null -w '%{url_effective}' https://github.com/ankitpokhrel/jira-cli/releases/latest | sed -E 's#.*/tag/##')" \
    && ver="${tag#v}" \
    && curl -fsSL "https://github.com/ankitpokhrel/jira-cli/releases/download/${tag}/jira_${ver}_linux_x86_64.tar.gz" -o /tmp/jira.tgz \
    && tar -xzf /tmp/jira.tgz -C /tmp \
    && install -m 0755 "/tmp/jira_${ver}_linux_x86_64/bin/jira" /usr/local/bin/jira \
    && rm -rf /tmp/jira*

RUN tag="$(curl -fsSL -o /dev/null -w '%{url_effective}' https://github.com/digitalocean/doctl/releases/latest | sed -E 's#.*/tag/##')" \
    && ver="${tag#v}" \
    && curl -fsSL "https://github.com/digitalocean/doctl/releases/download/${tag}/doctl-${ver}-linux-amd64.tar.gz" -o /tmp/doctl.tgz \
    && tar -xzf /tmp/doctl.tgz -C /tmp \
    && install -m 0755 /tmp/doctl /usr/local/bin/doctl \
    && rm -rf /tmp/doctl*

RUN curl -fsSL "https://storage.googleapis.com/cloud-sdk-release/google-cloud-cli-linux-x86_64.tar.gz" -o /tmp/gcloud.tgz \
    && tar -xzf /tmp/gcloud.tgz -C /opt \
    && rm /tmp/gcloud.tgz \
    && /opt/google-cloud-sdk/install.sh --quiet --usage-reporting=false --path-update=false --command-completion=false \
    && for b in gcloud gsutil bq; do ln -sf "/opt/google-cloud-sdk/bin/$b" "/usr/local/bin/$b"; done

# --- Node-based CLIs as the codespace user (uses the image's nvm Node) -------
# Claude Code plus the npm-delivered org CLIs. Installed under codespace's Node,
# then symlinked onto /usr/local/bin so a non-login `docker exec` finds them.
USER codespace
RUN bash -lc 'npm install -g @anthropic-ai/claude-code wrangler @bunny.net/cli'
USER root
RUN node_dir="$(runuser -l codespace -c 'command -v node' | xargs dirname)" \
    && for b in node npm npx corepack yarn pnpm claude wrangler bunny; do \
         [ -x "$node_dir/$b" ] && ln -sf "$node_dir/$b" "/usr/local/bin/$b"; \
       done; true

# --- Playwright Chromium for Plunder's e2e suite (issue #215) -----------------
# Plunder's `yarn test:e2e` drives headless Chromium through Playwright. Baking
# the browser plus its OS libraries into the image lets an agent reproduce and fix
# e2e failures on a fresh workspace immediately, instead of spending minutes on
# `playwright install --with-deps chromium` every run (a ~114MB browser plus the
# apt libs headless-shell needs, e.g. libatk-1.0.so.0, which otherwise fails to
# launch with "error while loading shared libraries").
#
# Browsers live in a shared, world-readable path (the official Playwright-in-Docker
# convention) and the matching env var is baked in, so Plunder's own Playwright
# finds the baked build no matter which user runs it. Chromium builds are pinned to
# a Playwright version, so we track the version Plunder uses; if Plunder later bumps
# Playwright, only the small browser re-downloads on first run (into this writable
# path), while the slow, root-only apt dependency step here always applies.
ENV PLAYWRIGHT_BROWSERS_PATH=/ms-playwright
ARG PLAYWRIGHT_VERSION=1.61.0
RUN npx -y "playwright@${PLAYWRIGHT_VERSION}" install --with-deps chromium \
    && chmod -R a+w /ms-playwright \
    && rm -rf /var/lib/apt/lists/*

# --- Seraphim agent helpers (post to the API; env injected per turn) ---------
COPY seraphim-suggest /usr/local/bin/seraphim-suggest
COPY seraphim-ask /usr/local/bin/seraphim-ask
COPY seraphim-comment /usr/local/bin/seraphim-comment
COPY seraphim-state /usr/local/bin/seraphim-state
COPY seraphim-draft /usr/local/bin/seraphim-draft
RUN chmod +x /usr/local/bin/seraphim-suggest /usr/local/bin/seraphim-ask \
    /usr/local/bin/seraphim-comment /usr/local/bin/seraphim-state \
    /usr/local/bin/seraphim-draft

# --- Computed-style check library for visual self-review (issue #245) ---------
# The deterministic CSS checks (centering/spacing/overflow/stacking) the agent
# runs through the Playwright MCP `browser_evaluate` to verify a layout before
# spending tokens on a screenshot. Baked at a fixed, world-readable path so the
# agent's default instructions can point at it no matter which repo it is on.
COPY visual-checks.md /usr/local/share/seraphim/visual-checks.md

# --- Visual regression baseline recipe (issue #246) ---------------------------
# The opt-in, per-repo recipe for committed Playwright `toHaveScreenshot()`
# baselines (standalone runner, not the MCP) plus anti-flake guidance, so the
# agent can lock a confirmed-good screen against future regressions. Baked at the
# same fixed path the default instructions point at; baselines themselves live in
# each opted-in repo, never here.
COPY visual-regression.md /usr/local/share/seraphim/visual-regression.md

# --- Throwaway PostgreSQL 17 helper (boots a local cluster for DB validation) -
COPY pg-ephemeral /usr/local/bin/pg-ephemeral
RUN chmod +x /usr/local/bin/pg-ephemeral

COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
# The container idles as root; the API runs Claude as `codespace` via docker exec.
CMD ["tail", "-f", "/dev/null"]
