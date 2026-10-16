# syntax=docker/dockerfile:1

# Toolchain Rust (version et composants de rust-toolchain.toml). Sert aussi de conteneur de dev (« toolbox »).
FROM rust:1.99-alpine3.24 AS toolchain
WORKDIR /src
COPY rust-toolchain.toml .
RUN rustup toolchain install

# Appli web (Expo) : export statique, servi par le binaire. Node ne sert qu'au build, pas dans l'image finale.
FROM node:24-alpine AS web
WORKDIR /src
COPY package.json package-lock.json ./
COPY apps/app/package.json apps/app/
COPY packages/api/package.json packages/api/
COPY packages/domain/package.json packages/domain/
RUN --mount=type=cache,target=/root/.npm npm ci --no-audit --no-fund
COPY apps apps
COPY packages packages
RUN npm run build:web

# Binaire statique (musl) pour l'architecture de la machine de build : amd64 en CI, arm64 sur un runner arm.
FROM toolchain AS build
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked --bin chaudron \
 && cp target/release/chaudron /chaudron

FROM scratch
LABEL org.opencontainers.image.source="https://github.com/MonChaudron/chaudron" \
      org.opencontainers.image.licenses="AGPL-3.0-only"
COPY --from=build /chaudron /chaudron
COPY --from=web /src/apps/app/dist /web
ENV PORT=8080 WEB_DIR=/web
USER 10001:10001
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s CMD ["/chaudron", "healthcheck"]
ENTRYPOINT ["/chaudron"]
