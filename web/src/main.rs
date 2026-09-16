#![allow(non_snake_case)]
use dioxus::prelude::*;
use futures::{
    future::{select, Either},
    SinkExt, StreamExt,
};
use gloo_net::websocket::{futures::WebSocket, Message};
use gloo_timers::future::TimeoutFuture;
use material_dioxus::{
    palette::*,
    text_inputs::{MatTextArea, TextAreaCharCounter},
    theming::{Colors, MatTheme},
};

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

fn ws_url() -> String {
    let location = web_sys::window().unwrap().location();
    let scheme = if location.protocol().unwrap() == "https:" {
        "wss"
    } else {
        "ws"
    };
    format!("{}://{}/ws", scheme, location.host().unwrap())
}

fn App(cx: Scope) -> Element {
    let value = use_state(cx, String::new);
    let connected = use_state(cx, || false);
    let tx = use_coroutine(cx, |mut rx: UnboundedReceiver<String>| {
        to_owned![value, connected];
        async move {
            'reconnect: loop {
                match WebSocket::open(&ws_url()) {
                    Ok(ws) => {
                        connected.set(true);
                        let (mut write, mut read) = ws.split();
                        loop {
                            match select(read.next(), rx.next()).await {
                                Either::Left((msg, _)) => match msg {
                                    Some(Ok(Message::Text(text))) => value.set(text),
                                    Some(Ok(_)) => {}
                                    _ => break,
                                },
                                Either::Right((Some(text), _)) => {
                                    if write.send(Message::Text(text)).await.is_err() {
                                        break;
                                    }
                                }
                                Either::Right((None, _)) => break 'reconnect,
                            }
                        }
                        connected.set(false);
                    }
                    Err(_) => connected.set(false),
                }
                TimeoutFuture::new(1_000).await;
            }
        }
    });
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
        MatTextArea{
            value: "{value}",
            label: "Clipboard",
            style: "width: 100%; height: calc(95svh - 2rem)",
            outlined: true,
            max_length: MAX_SIZE as u64,
            disabled: !connected,
            char_counter: TextAreaCharCounter::External,
            _oninput: {
                to_owned![value, tx];
                move |new_value: String| {
                    tx.send(new_value.clone());
                    value.set(new_value);
                }
            }
        }
        if !connected {
            rsx! {
                div {
                    color: "var(--mdc-theme-error)",
                    "Reconnecting"
                }
            }
        }
    }
}
