# Webclip

Webclip is a simple cross-device web-based clipboard written in
[Actix](https://actix.rs/) and [Dioxus](https://dioxuslabs.com/).

## How to Run

### Docker

```sh
docker run -d -p 9257:9257 ghcr.io/gobidev/webclip:main
```

### Manually

- Install [`trunk`](https://trunkrs.dev/), [`npm`](https://www.npmjs.com/),
  [`cargo`](https://rustup.rs/) and
  [binaryen](https://github.com/WebAssembly/binaryen).
- Inside the `web` directory, run `trunk build --release`
- Inside the root directory, run `cargo run --profile=backend`
- The webserver will run on port `9257`

## Configuration

For both compilations, you can set this environment variable:

- `WEBCLIP_MAX_SIZE` to specify the maximum allowed size for the clipboard,
  default is 100,000 characters.

You can configure the address and port during runtime with these environment
variables:

- `WEBCLIP_BIND_ADDRESS`: which address to bind to, default is `0.0.0.0`.
- `WEBCLIP_BIND_PORT`: which port to bind to, default is `9257`.

## Development

For development, install the dependencies as described above, then run
`cargo run` in the root directory and `trunk serve` in the `web` directory.
