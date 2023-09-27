# Webclip

Webclip is a simple cross-device web-based clipboard written in
[Actix](https://actix.rs/) and [Dioxus](https://dioxuslabs.com/).

## How to Run

- Install [`trunk`](https://trunkrs.dev/), [`npm`](https://www.npmjs.com/) and
  [`cargo`](https://rustup.rs/)
- Inside the `web` directory, run `trunk build --release`
- Inside the root directory, run `cargo run --release`
- The webserver will run on port `9257`

## Configuration

For both compilations, you can set this environment variable:

- `WEBCLIP_MAX_SIZE` to specify the maximum allowed size for the clipboard

You can configure the address and port during runtime with these environment
variables:

- `WEBCLIP_BIND_ADDRESS`: which address to bind to
- `WEBCLIP_BIND_PORT`: which port to bind to

## Development

For development, install the dependencies as described above, then run
`cargo run` in the root directory and `trunk serve` in the `web` directory.
