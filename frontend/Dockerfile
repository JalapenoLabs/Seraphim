# --- Build stage -------------------------------------------------------------
FROM node:22-bookworm-slim AS build
WORKDIR /app

# Install deps first for layer caching; only re-runs when package.json changes.
COPY package.json ./
RUN npm install

COPY svelte.config.js vite.config.ts tsconfig.json ./
COPY src/ ./src/
COPY static/ ./static/
RUN npm run build

# --- Runtime stage -----------------------------------------------------------
FROM node:22-bookworm-slim AS runtime
WORKDIR /app
ENV NODE_ENV=production

COPY package.json ./
RUN npm install --omit=dev

COPY --from=build /app/build ./build

ENV PORT=31415
EXPOSE 31415
CMD ["node", "build"]
