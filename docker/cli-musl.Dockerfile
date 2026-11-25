# syntax=docker/dockerfile:1.10
ARG ALPINE_IMAGE=alpine:3.22
ARG VERIFY_IMAGE=debian:bookworm-slim
FROM ${ALPINE_IMAGE} AS build
ARG RUST_TOOLCHAIN
RUN apk add --no-cache bash build-base coreutils file just rustup \
    && rustup-init -y --no-modify-path --profile minimal --default-toolchain "${RUST_TOOLCHAIN}"
ENV PATH="/root/.cargo/bin:${PATH}" \
    JUST_UNSTABLE=1
WORKDIR /src
COPY Cargo.toml Cargo.lock justfile LICENSE ./
COPY crates crates
COPY schema schema
RUN cargo build --release --locked --package opcov-cli \
    && file target/release/opcov | grep -q 'static' \
    && version="$(cargo pkgid --package opcov-cli)" \
    && case "$(uname -m)" in x86_64) arch=x86_64 ;; aarch64) arch=arm64 ;; *) echo "unsupported machine $(uname -m)" >&2; exit 1 ;; esac \
    && just dist-cli target/release/opcov "${version##*[#@]}" "${arch}-linux-musl" /out

FROM ${VERIFY_IMAGE} AS verify
COPY --from=build /out /out
RUN cd /out && sha256sum --check --strict ./*.sha256 \
    && for asset in ./*.sha256; do "./${asset%.sha256}" --version; done

FROM scratch
COPY --from=verify /out /
