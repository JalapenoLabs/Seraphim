# --- Build stage -------------------------------------------------------------
FROM node:22-bookworm-slim AS build
WORKDIR /app
RUN corepack enable
ENV COREPACK_ENABLE_DOWNLOAD_PROMPT=0

# Install deps first for layer caching; only re-runs when the lockfile changes.
COPY package.json yarn.lock .yarnrc.yml ./
RUN yarn install --immutable

COPY svelte.config.js vite.config.ts tsconfig.json ./
COPY src/ ./src/
COPY static/ ./static/
RUN yarn build

# --- Runtime stage -----------------------------------------------------------
FROM node:22-bookworm-slim AS runtime
WORKDIR /app
ENV NODE_ENV=production
RUN corepack enable
ENV COREPACK_ENABLE_DOWNLOAD_PROMPT=0

# adapter-node externalizes deps, so the build output still needs them present.
COPY package.json yarn.lock .yarnrc.yml ./
RUN yarn install --immutable

COPY --from=build /app/build ./build

ENV PORT=31415
EXPOSE 31415
CMD ["node", "build"]
