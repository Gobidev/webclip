FROM rust:alpine AS builder

RUN apk add musl-dev build-base

COPY . /webclip

WORKDIR /webclip
RUN cargo build --profile=backend --locked

FROM alpine:latest

RUN adduser -D -H app

WORKDIR /webclip
COPY --from=builder /webclip/target/backend/webclip /usr/local/bin/webclip
COPY --from=builder /webclip/web/static ./web/static

USER app
EXPOSE 9257
CMD ["webclip"]
