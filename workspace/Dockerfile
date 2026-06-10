# The agent's workspace sandbox.
#
# A long-lived container with everything Claude Code needs to act like a
# developer: the CLI itself, git, the GitHub CLI, a Docker client (talking to the
# mounted host socket), Earthly, Node, and passwordless sudo. The API drives it
# via `docker exec`; the container itself just idles.

FROM node:22-bookworm

# Static Docker client version pulled below (client only; talks to host socket).
ARG DOCKER_VERSION=27.3.1

# --- Base tooling (stable layer) ---------------------------------------------
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates curl gnupg git jq sudo less ripgrep openssh-client \
    && rm -rf /var/lib/apt/lists/*

# --- GitHub CLI --------------------------------------------------------------
RUN curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg \
        -o /usr/share/keyrings/githubcli-archive-keyring.gpg \
    && echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" \
        > /etc/apt/sources.list.d/github-cli.list \
    && apt-get update \
    && apt-get install -y --no-install-recommends gh \
    && rm -rf /var/lib/apt/lists/*

# --- Docker client (static binary) -------------------------------------------
RUN curl -fsSL "https://download.docker.com/linux/static/stable/x86_64/docker-${DOCKER_VERSION}.tgz" \
        | tar xz --strip-components=1 -C /usr/local/bin docker/docker

# --- Earthly -----------------------------------------------------------------
RUN curl -fsSL https://github.com/earthly/earthly/releases/latest/download/earthly-linux-amd64 \
        -o /usr/local/bin/earthly \
    && chmod +x /usr/local/bin/earthly

# --- Claude Code CLI ---------------------------------------------------------
RUN npm install -g @anthropic-ai/claude-code

# --- pnpm via corepack -------------------------------------------------------
# Enable the corepack pnpm shim here (as root) so repo setup scripts never need
# `sudo corepack enable`, which fails for the non-root `node` user (it can't
# symlink into the root-owned /usr/local/bin). Classic yarn ships in the base
# image already; projects that pin a pnpm version get it on first use.
RUN corepack enable pnpm

# --- Passwordless sudo for the agent (trusted sandbox) -----------------------
RUN echo "node ALL=(ALL) NOPASSWD:ALL" > /etc/sudoers.d/node && chmod 0440 /etc/sudoers.d/node

# --- seraphim-suggest: the agent's "recommend a setup change" helper ---------
# A dependency-free Node CLI (uses Node 22's global fetch) that posts setup
# recommendations to the API. The orchestrator injects SERAPHIM_API_URL and
# SERAPHIM_TASK_ID into the turn's exec environment.
COPY seraphim-suggest /usr/local/bin/seraphim-suggest
RUN chmod +x /usr/local/bin/seraphim-suggest

COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
# The container idles; the API runs Claude via `docker exec`.
CMD ["tail", "-f", "/dev/null"]
