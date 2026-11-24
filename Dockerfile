FROM rust:bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates cmake git ninja-build pkg-config python3 && rm -rf /var/lib/apt/lists/*
WORKDIR /web
COPY patches patches
RUN git init /pomme && git -C /pomme fetch --depth 1 https://github.com/PommeMC/Client.git "$(cat patches/BASE)" && git -C /pomme checkout --detach FETCH_HEAD && git -C /pomme submodule update --init
COPY rust/ /pomme/
RUN git -C /pomme apply /web/patches/pomme.patch
RUN toolchain="$(sed -n 's/^channel *= *"\(.*\)"/\1/p' /pomme/rust-toolchain.toml)" && rustup toolchain install "$toolchain" --profile minimal --target wasm32-unknown-unknown && rustup default "$toolchain"
RUN cargo install wasm-bindgen-cli --version 0.2.121 --locked
COPY build.sh .
COPY web/ web/
RUN POMME_REPO=/pomme ./build.sh
RUN cargo build --manifest-path /pomme/Cargo.toml --locked --release -p pomme-relay --features local-worlds

FROM debian:bookworm-slim
LABEL org.opencontainers.image.source="https://github.com/VKRTTY/PommeWeb"
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=build /web/dist/ ./dist/
COPY --from=build /pomme/target/release/pomme-relay /usr/local/bin/pomme-relay
COPY railway-start.sh /usr/local/bin/railway-start
USER 65534:65534
ENTRYPOINT ["/usr/local/bin/railway-start"]
