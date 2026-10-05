# Contexte de construction : racine du dépôt (voir docker-compose.yml).
FROM rust:1-slim-trixie AS build
WORKDIR /src
# 1) Dépendances seules (couche mise en cache tant que Cargo.lock ne change pas).
COPY gateway/Cargo.toml gateway/Cargo.lock gateway/
RUN mkdir -p gateway/src && echo 'fn main() {}' > gateway/src/main.rs && touch gateway/src/lib.rs \
 && cd gateway && cargo build --release --locked && rm -rf src
# 2) Code de la passerelle.
COPY db db
COPY gateway gateway
RUN cd gateway && touch src/main.rs src/lib.rs && cargo build --release --locked

FROM debian:trixie-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* && useradd --system --uid 10001 triage
COPY --from=build /src/gateway/target/release/triage-gateway /usr/local/bin/triage-gateway
COPY config /app/config
COPY samples /app/samples
RUN chmod -R a+rX /app
USER triage
ENV LISTEN_ADDR=0.0.0.0:8080 RULES_FILE=/app/config/rules.json SAMPLES_FILE=/app/samples/emails.json
EXPOSE 8080
HEALTHCHECK --interval=5s --timeout=3s --retries=20 CMD ["triage-gateway", "healthcheck"]
ENTRYPOINT ["triage-gateway"]
CMD ["serve"]
