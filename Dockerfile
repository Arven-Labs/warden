# A imagem do hub (`warden-server`), para a hospedagem e para o self-host em contêiner (P143, HOSPEDAGEM.md fase 1).
#
#   docker build -t warden-server .
#   docker run --read-only --tmpfs /tmp -v warden-data:/data -p 7420:7420 \
#     -e WARDEN_SERVER_AUTH_KEY="$(openssl rand -hex 32)" -e GEMINI_API_KEY=... warden-server
#
# A raiz pode ser só de leitura: tudo o que o hub grava vai para /data (`--data-dir`), e o /tmp em tmpfs é só folga para os
# processos filhos (`git`, servidores MCP). `scripts/image-smoke.sh` confere isso num contêiner de verdade.
# Construída no GitHub Actions (`.github/workflows/image.yml`), não na máquina de desenvolvimento.

# 1. O frontend: `web/dist` entra no binário por `rust-embed`, então precisa existir antes do `cargo build`.
FROM node:26-slim AS web
WORKDIR /src/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

# 2. O binário. Debian 13 (glibc 2.41): o ONNX Runtime que o `ort` baixa pede glibc 2.38 ou mais. `cmake` é do `aws-lc-sys`.
FROM rust:1-trixie AS build
RUN apt-get update && apt-get install -y --no-install-recommends cmake && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY . .
COPY --from=web /src/web/dist web/dist
RUN cargo build --release --locked -p warden-server

# 3. O que roda. `git` é do sync; `curl` é do HEALTHCHECK.
FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl git \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /tmp --shell /usr/sbin/nologin warden \
    && mkdir /data && chown warden /data
COPY --from=build /src/target/release/warden-server /usr/local/bin/warden-server

USER warden
# Com a raiz só de leitura, `$HOME` aponta para o /tmp (tmpfs); nada do Warden mora lá.
ENV HOME=/tmp
VOLUME /data
EXPOSE 7420

# `/health` é público e mínimo. Assume HTTP simples: um hub com TLS próprio troca esta sonda.
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 CMD curl -fsS http://127.0.0.1:7420/health || exit 1

# A chave de pareamento vem de WARDEN_SERVER_AUTH_KEY, sem valor padrão. `--no-embedding-warm-up` é o perfil hospedado
# (P146): o hub ocioso pesa ~25 MB em vez de ~960 MB, e cada modelo de busca carrega na primeira busca.
ENTRYPOINT ["warden-server", "--data-dir", "/data"]
CMD ["serve", "--no-embedding-warm-up"]
