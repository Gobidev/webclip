#![allow(non_snake_case)]
use dioxus::prelude::*;
use futures::{SinkExt, StreamExt};
use gloo_net::websocket::{futures::WebSocket, Message};
use material_dioxus::{
    palette::*,
    text_inputs::{MatTextArea, TextAreaCharCounter},
    theming::{Colors, MatTheme},
};
use once_cell::sync::Lazy;

static CLIENT: Lazy<reqwest::Client> = Lazy::new(reqwest::Client::new);
static BACKEND_URL: Lazy<String> =
    Lazy::new(|| web_sys::window().unwrap().location().origin().unwrap());

const fn parse_int(s: &str) -> usize {
    let mut bytes = s.as_bytes();
    let mut val = 0;
    while let [byte, rest @ ..] = bytes {
        assert!(b'0' <= *byte && *byte <= b'9', "invalid digit");
        val = val * 10 + (*byte - b'0') as usize;
        bytes = rest;
    }
    val
}

const MAX_SIZE: usize = match option_env!("WEBCLIP_MAX_SIZE") {
    Some(size) => parse_int(size),
    None => 100_000,
};

const GRUVBOX_BG: Color = from_u32(0x282828, 1.);
const GRUVBOX_FG: Color = from_u32(0xebdbb2, 1.);
const GRUVBOX_GREEN: Color = from_u32(0x98971a, 1.);
const GRUVBOX_RED: Color = from_u32(0xfb4934, 1.);

fn main() {
    dioxus_web::launch(App)
}

fn App(cx: Scope) -> Element {
    let request_sent = use_state(cx, || false);
    let fetched = use_state(cx, || false);
    let value = use_state(cx, String::new);
    let error = use_state(cx, String::new);
    let dont_update = use_state(cx, || false);
    let ws_connected = use_state(cx, || false);
    let tx = use_coroutine(cx, |mut rx: UnboundedReceiver<String>| {
        to_owned![value, ws_connected, dont_update];
        async move {
            let ws = match WebSocket::open(&(BACKEND_URL.replace("http", "ws") + "/ws")) {
                Ok(w) => w,
                Err(_) => return,
            };
            ws_connected.set(true);
            let (mut write, mut read) = ws.split();
            loop {
                tokio::select! {
                    Some(Ok(Message::Text(m))) = read.next() => {
                        value.set(m);
                        dont_update.set(true);
                    },
                    Some(m) = rx.next() => drop(write.send(Message::Text(m)).await),
                };
            }
        }
    });
    if !request_sent {
        cx.spawn({
            to_owned![value, fetched, error];
            request_sent.set(true);
            async move {
                match CLIENT
                    .get(format!("{}/clipboard", &*BACKEND_URL))
                    .send()
                    .await
                {
                    Ok(response) => {
                        let status = response.status();
                        match response.text().await {
                            Ok(text) if status.is_success() => value.set(text),
                            Ok(text) => error.set(text),
                            Err(err) => error.set(err.to_string()),
                        }
                    }
                    Err(err) => error.set(err.to_string()),
                }
                fetched.set(true);
            }
        });
    }
    if **fetched && !dont_update {
        if **ws_connected {
            tx.send(value.get().clone())
        } else {
            to_owned![value];
            cx.spawn(async move {
                CLIENT
                    .post(format!("{}/clipboard", &*BACKEND_URL))
                    .body(value.get().clone())
                    .send()
                    .await
                    .unwrap();
            });
        }
    }
    render! {
        style {
            dangerous_inner_html: "
                body {{
                    background-color: var(--mdc-theme-background);
                    margin: 1rem;
                    font-family: Roboto;
                }}

                html {{
                    color-scheme: dark;
                }}
            "
        }
        MatTheme{
            theme: Colors{ background: GRUVBOX_BG, on_surface: Some(GRUVBOX_FG), primary: GRUVBOX_GREEN, error: GRUVBOX_RED, ..Colors::DEFAULT_DARK },
            dark_theme: None,
        }
        if error.is_empty() {
            rsx! {
                MatTextArea{
                    value: "{value}",
                    label: "Clipboard",
                    style: "width: 100%; height: calc(95svh - 2rem)",
                    outlined: true,
                    max_length: MAX_SIZE as u64,
                    disabled: !fetched,
                    char_counter: TextAreaCharCounter::External,
                    _oninput: {
                        to_owned![value, dont_update];
                        move |new_value| {
                            value.set(new_value);
                            dont_update.set(false);
                        }
                    }
                }
            }
        } else {
            rsx! {
                div {
                    color: "var(--mdc-theme-error)",
                    "{error}"
                }
            }
        }
    }
}
