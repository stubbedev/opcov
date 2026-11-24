# syntax=docker/dockerfile:1.10
ARG PHP_IMAGE
FROM ${PHP_IMAGE} AS build
ARG RUST_TOOLCHAIN
RUN apk add --no-cache bash build-base clang-dev just rustup tar \
    && rustup-init -y --no-modify-path --profile minimal --default-toolchain "${RUST_TOOLCHAIN}" \
    && docker-php-source extract \
    && mkdir -p /opt/static-libgcc \
    && echo 'INPUT(-lgcc_eh -lgcc)' > /opt/static-libgcc/libgcc_s.so
ENV PATH="/root/.cargo/bin:${PATH}" \
    PHP_CONFIG=php-config \
    PHP_RUN_TESTS=/usr/src/php/run-tests.php \
    RUSTFLAGS="-C target-feature=-crt-static -L native=/opt/static-libgcc"
WORKDIR /src
COPY Cargo.toml Cargo.lock justfile LICENSE ./
COPY crates crates
COPY schema schema
COPY tests/phpt tests/phpt
RUN cargo build --release --locked --package opcov-ext --features engine-selftest \
    && just phpt-run target/release/libopcov_ext.so tests/phpt \
    && cargo build --release --locked --package opcov-ext \
    && version="$(cargo pkgid --package opcov-ext)" \
    && just dist target/release/libopcov_ext.so "${version##*[#@]}" musl /out

FROM ${PHP_IMAGE} AS verify
COPY --from=build /out /out
RUN mkdir /tmp/opcov \
    && for archive in /out/*.tgz; do tar -xzf "$archive" -C /tmp/opcov; done \
    && php -n -d extension=/tmp/opcov/opcov.so -r 'echo \opcov\version(), PHP_EOL;'

FROM scratch
COPY --from=verify /out /
