FROM rust:alpine

RUN apk add npm binaryen pkgconfig openssl-dev musl-dev build-base curl

RUN curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | sh

RUN cargo binstall trunk

RUN rustup target add wasm32-unknown-unknown

COPY . /webclip

WORKDIR /webclip/web
RUN trunk build --release

WORKDIR /webclip
RUN cargo build --profile=backend

EXPOSE 9257
CMD ["cargo", "run", "--profile=backend"]
