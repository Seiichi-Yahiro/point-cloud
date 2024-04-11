#[cfg(not(target_arch = "wasm32"))]
pub type LoadError = std::io::Error;

#[cfg(target_arch = "wasm32")]
pub type LoadError = js_sys::Error;

#[cfg(not(target_arch = "wasm32"))]
pub fn no_source_error() -> LoadError {
    std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "No source to load from provided",
    )
}

#[cfg(target_arch = "wasm32")]
pub fn no_source_error() -> LoadError {
    let err = js_sys::Error::new("No source to load from provided");
    err.set_cause(&wasm_bindgen::JsValue::from_str("NotFoundError"));
    err
}
