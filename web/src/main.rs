#![allow(non_snake_case)]
use dioxus::prelude::*;
use material_dioxus::{
    palette::*,
    text_inputs::{MatTextArea, TextAreaCharCounter},
    theming::{Colors, MatTheme},
};
use once_cell::sync::Lazy;

static CLIENT: Lazy<reqwest::Client> = Lazy::new(reqwest::Client::new);
static BACKEND_URL: Lazy<String> =
    Lazy::new(|| web_sys::window().unwrap().location().origin().unwrap());

fn main() {
    dioxus_web::launch(App)
}

fn App(cx: Scope) -> Element {
    let request_sent = use_state(cx, || false);
    let fetched = use_state(cx, || false);
    let value = use_state(cx, String::new);
    let error = use_state(cx, String::new);
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
    if **fetched {
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
    render! {
        style {
            dangerous_inner_html: "
                body {{
                    background-color: var(--mdc-theme-background);
                    margin: 1rem;
                    font-family: Roboto;
                }}

                html {{
                    color-scheme: light dark;
                }}
            "
        }
        MatTheme{
            theme: Colors{ primary: LIGHT_GREEN_500, error: RED_500, ..Colors::DEFAULT_DARK },
            dark_theme: None,
        }
        if error.is_empty() {
            rsx! {
                MatTextArea{
                    value: "{value}",
                    label: "Clipboard",
                    style: "width: 100%; height: calc(95svh - 2rem)",
                    outlined: true,
                    max_length: 100_000,
                    disabled: !fetched,
                    char_counter: TextAreaCharCounter::External,
                    _oninput: {
                        to_owned![value];
                        move |new_value| {
                            value.set(new_value);
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
