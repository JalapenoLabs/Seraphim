# The agent's workspace sandbox (issue #77).
#
# Based on Microsoft's universal devcontainer image: Python, Node (via nvm), Go,
# Java, .NET, PHP, Ruby, plus version managers, out of the box. On top we add the
# tools the agents need that it doesn't ship: Rust/Cargo, the Claude Code CLI, the
# GitHub CLI, a Docker client, Earthly, and the org cloud CLIs (jira, doctl,
# gcloud, wrangler, bunny). Work runs as the image's non-root `codespace` user;
# the API drives it via `docker exec`, the container itself just idles.

FROM mcr.microsoft.com/devcontainers/universal:latest

# Static Docker client version pulled below (client only; talks to host socket).
ARG DOCKER_VERSION=27.3.1
USER root

# --- Base CLIs we rely on (most already present; ensure the rest) ------------
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates curl gnupg git jq sudo less ripgrep openssh-client \
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

# --- Seraphim agent helpers (post to the API; env injected per turn) ---------
COPY seraphim-suggest /usr/local/bin/seraphim-suggest
COPY seraphim-ask /usr/local/bin/seraphim-ask
RUN chmod +x /usr/local/bin/seraphim-suggest /usr/local/bin/seraphim-ask

COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
# The container idles as root; the API runs Claude as `codespace` via docker exec.
CMD ["tail", "-f", "/dev/null"]
