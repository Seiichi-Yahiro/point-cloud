use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

#[derive(Debug, Clone)]
pub struct WebDir(web_sys::FileSystemDirectoryHandle);

impl WebDir {
    pub async fn choose() -> Result<Self, JsValue> {
        let window = web_sys::window().unwrap();
        let dir = JsFuture::from(window.show_directory_picker()?)
            .await?
            .dyn_into::<web_sys::FileSystemDirectoryHandle>()?;

        Ok(Self(dir))
    }

    pub async fn get_dir_handle(&self, dir_name: &str) -> Result<WebDir, JsValue> {
        let dir = JsFuture::from(self.0.get_directory_handle(dir_name))
            .await?
            .dyn_into::<web_sys::FileSystemDirectoryHandle>()?;

        Ok(Self(dir))
    }

    pub async fn get_file_handle(&self, file_name: &str) -> Result<WebFile, JsValue> {
        let file = JsFuture::from(self.0.get_file_handle(file_name))
            .await?
            .dyn_into::<web_sys::FileSystemFileHandle>()?;

        Ok(WebFile(file))
    }
}

#[derive(Debug, Clone)]
pub struct WebFile(web_sys::FileSystemFileHandle);

impl WebFile {
    pub async fn read_bytes(&self) -> Result<Vec<u8>, JsValue> {
        let file = self.get_file().await?;

        let array_buffer = JsFuture::from(file.array_buffer())
            .await?
            .dyn_into::<js_sys::ArrayBuffer>()?;

        Ok(js_sys::Uint8Array::new(&array_buffer).to_vec())
    }

    async fn get_file(&self) -> Result<web_sys::File, JsValue> {
        JsFuture::from(self.0.get_file())
            .await?
            .dyn_into::<web_sys::File>()
    }
}
