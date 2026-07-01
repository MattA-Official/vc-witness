## ---- Build stage -----------------------------------------------------------
FROM rust:1-bookworm AS builder

# whisper-rs vendors and compiles whisper.cpp via cmake+bindgen; songbird's opus decoder is
# likewise compiled from vendored C source. All three need a C/C++ toolchain and libclang.
RUN apt-get update && apt-get install -y --no-install-recommends \
    cmake \
    clang \
    libclang-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

# ggml's CMake build defaults to `-march=native`, tuning for whatever CPU the image happens to
# be built on. That produces a binary that can SIGILL on a different (e.g. older) host CPU, which
# would be silent and confusing for anyone pulling a prebuilt image. Force a portable baseline.
ENV GGML_NATIVE=OFF

WORKDIR /app

# Cache dependency (and vendored whisper.cpp/opus) builds separately from application code, so
# editing src/ doesn't force a full native rebuild.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo build --release --locked \
    && rm -rf src

COPY src ./src
COPY migrations ./migrations
# sqlx::migrate!() embeds migration files into the binary at compile time via the touched
# main.rs above; touch it again so cargo actually recompiles the real sources.
RUN touch src/main.rs && cargo build --release --locked

## ---- Runtime stage ----------------------------------------------------------
FROM debian:bookworm-slim

# libstdc++6: whisper.cpp is C++ and links against it dynamically.
# ca-certificates: TLS to Discord's gateway/API.
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libstdc++6 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/vc-witness /usr/local/bin/vc-witness

# Runtime state: SQLite DB and the Whisper model both live under mounted volumes so they
# survive image upgrades and don't bloat the image itself.
VOLUME ["/data", "/models"]
ENV DATABASE_PATH=/data/witness.sqlite3

ENTRYPOINT ["/usr/local/bin/vc-witness"]
