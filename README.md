# Webclip

Webclip is a simple cross-device web-based clipboard written in
[Actix](https://actix.rs/) with a dependency-free JavaScript frontend. Next to
the shared clipboard it offers private clipboards: the same clipboard shared
only with the peers that know its PIN, deleted after inactivity.

## How to Run

### Docker

```sh
docker run -d -p 9257:9257 ghcr.io/gobidev/webclip:main
```

### Manually

- Install [`cargo`](https://rustup.rs/).
- Inside the root directory, run `cargo run --profile=backend`.
- The webserver will run on port `9257` and serve the frontend from
  `web/static`.

## Configuration

`WEBCLIP_MAX_SIZE` sets the maximum allowed size for the clipboard (in
characters). It is read at compile time (default `100000`) and is also
exposed to the frontend through `/config.js`.

The clipboard also clears itself after 12 hours without any changes. This
lifetime is currently fixed and is checked once a minute; when it expires the
content is emptied for all connected clients.

You can configure the address and port during runtime with these environment
variables:

- `WEBCLIP_BIND_ADDRESS`: which address to bind to, default is `0.0.0.0`.
- `WEBCLIP_BIND_PORT`: which port to bind to, default is `9257`.

The theme colors are also read at runtime (no rebuild needed). Any CSS color
value is accepted, and the foreground's translucent variants (outline, label,
counter) are derived automatically:

- `WEBCLIP_COLOR_BACKGROUND`: page background, default `#282828`.
- `WEBCLIP_COLOR_FOREGROUND`: text and outline base color, default `#ebdbb2`.
- `WEBCLIP_COLOR_PRIMARY`: focus/accent color, default `#98971a`.
- `WEBCLIP_COLOR_ERROR`: error color, default `#fb4934`.

All of the runtime variables above can also be placed in a `.env` file in the
working directory (loaded at startup). `WEBCLIP_MAX_SIZE` is the exception: it
is read at compile time. Values starting with `#` must be quoted, since `#`
begins a comment in `.env` files:

```sh
WEBCLIP_COLOR_BACKGROUND="#282828"
WEBCLIP_COLOR_PRIMARY="#98971a"
WEBCLIP_BIND_PORT=9257
WEBCLIP_PRIVATE_TTL=1800
```

For example:

```sh
docker run -d -p 9257:9257 \
  -e WEBCLIP_COLOR_BACKGROUND='#1d2021' \
  -e WEBCLIP_COLOR_FOREGROUND='#d5c4a1' \
  -e WEBCLIP_COLOR_PRIMARY='#83a598' \
  -e WEBCLIP_COLOR_ERROR='#fb4934' \
  ghcr.io/gobidev/webclip:main
```

## Private Clipboards

The "Private" tab opens a clipboard that is shared only with the peers who know
its PIN. Press `Create` to get a generated PIN (4 digits by default) and enter
the room; other devices enter that PIN and press `Join`. Once connected, the
private clipboard behaves exactly like the shared one: every peer sees changes
live and can copy, clear or leave the room. The PIN is shown next to `Leave`
and can be copied with a click, so more peers can join later.

Rooms are held in memory only and are deleted once nobody is connected and
nothing has changed for 30 minutes. Neither their contents nor their PINs are
written to the logs. Like the shared clipboard they are only as private as the
server they run on: contents are stored in plain text and the PIN is sent to
the server, so use HTTPS or a trusted network. Failed joins are rate limited
per client IP, and a longer PIN makes guessing harder. The following variables
can be set at runtime (or in `.env`):

| Variable | Default | Meaning |
| --- | --- | --- |
| `WEBCLIP_PRIVATE_ENABLED` | `true` | Set to `false` to hide the tab and disable the endpoints |
| `WEBCLIP_PRIVATE_TTL` | `1800` | Seconds of inactivity before an empty room is deleted (minimum 30) |
| `WEBCLIP_PRIVATE_PIN_LENGTH` | `4` | Number of PIN digits (between 3 and 12) |
| `WEBCLIP_PRIVATE_MAX_ROOMS` | `100` | Maximum number of rooms kept in memory |
| `WEBCLIP_PRIVATE_MAX_ATTEMPTS` | `10` | Failed joins and room creations per client IP per minute |

A server restart empties the private rooms as well as the shared clipboard.
`/config.js` exposes the private settings to the frontend so the tab and PIN
field match the server configuration.

## Logging

Logging verbosity is controlled by `RUST_LOG` (default `info`). At the default
level the server logs every WebSocket connect/disconnect with the client IP and
User-Agent, plus a periodic status line (every 60s, when there are clients,
clipboard content or private rooms) summarizing connected clients, how full the
clipboard is and how many private rooms and peers are active. Individual
clipboard updates are logged at `debug`, so use `RUST_LOG=debug` to see every
change.

## Development

Run `cargo run --profile=backend` in the root directory. The frontend lives in
`web/static` (`index.html`, `style.css`, `app.js`) and is served as static
files, so no build step or external tooling is required. The backend sends the
current clipboard to every client as soon as it connects and broadcasts every
change to all other connected clients over a WebSocket at `/ws`.
