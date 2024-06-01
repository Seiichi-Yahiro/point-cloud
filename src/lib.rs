pub use app::App;

mod app;
mod event_set;
mod plugins;
mod sorted_hash;
mod texture;
mod transform;

#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub async fn run(canvas_id: String, url: Option<String>) {
    use std::str::FromStr;
    use url::Url;

    App {
        canvas_id: Some(canvas_id),
        url: url.map(|url| Url::from_str(&url).unwrap()),
    }
    .run()
    .await;
}
