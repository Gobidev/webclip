FROM rust:alpine AS builder

RUN apk add npm binaryen musl-dev build-base curl

RUN curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | sh

RUN cargo binstall trunk

RUN rustup target add wasm32-unknown-unknown

COPY . /webclip

WORKDIR /webclip/web
RUN trunk build --release

WORKDIR /webclip
RUN cargo build --profile=backend --locked

FROM alpine:latest

RUN adduser -D -H app

WORKDIR /webclip
COPY --from=builder /webclip/target/backend/webclip /usr/local/bin/webclip
COPY --from=builder /webclip/web/dist ./web/dist

USER app
EXPOSE 9257
CMD ["webclip"]
