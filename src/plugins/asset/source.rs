use crate::plugins::asset::Asset;

#[cfg(not(target_arch = "wasm32"))]
pub type LoadError = std::io::Error;

#[cfg(target_arch = "wasm32")]
pub type LoadError = js_sys::Error;

#[derive(Debug, Clone)]
pub struct LoadErrorKind {
    #[cfg(not(target_arch = "wasm32"))]
    kind: std::io::ErrorKind,

    #[cfg(target_arch = "wasm32")]
    kind: String,
}

#[cfg(not(target_arch = "wasm32"))]
impl AsRef<std::io::ErrorKind> for LoadErrorKind {
    fn as_ref(&self) -> &std::io::ErrorKind {
        &self.kind
    }
}

#[cfg(target_arch = "wasm32")]
impl AsRef<str> for LoadErrorKind {
    fn as_ref(&self) -> &str {
        &self.kind
    }
}

impl From<LoadError> for LoadErrorKind {
    fn from(error: LoadError) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        return Self { kind: error.kind() };

        #[cfg(target_arch = "wasm32")]
        return Self {
            kind: error.name().as_string().unwrap(),
        };
    }
}

#[derive(Debug, Clone)]
pub enum Source {
    #[cfg(not(target_arch = "wasm32"))]
    Path(std::path::PathBuf),

    #[cfg(target_arch = "wasm32")]
    PathInDirectory {
        directory: crate::web::WebDir,
        path: std::path::PathBuf,
    },

    URL(String),
}

impl Source {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load<T: Asset>(&self) -> Result<T, LoadError> {
        match self {
            Source::Path(path) => {
                let file = std::fs::File::open(path)?;
                let mut buf_reader = std::io::BufReader::new(file);
                T::from_reader(&mut buf_reader)
            }
            Source::URL(_) => {
                todo!()
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn load<T: Asset>(&self) -> Result<T, LoadError> {
        match self {
            Source::PathInDirectory { directory, path } => {
                use std::path::Component;
                use wasm_bindgen::JsValue;

                let mut dir = directory.clone();

                let components = path.components().collect::<Vec<_>>();

                for (i, component) in components.iter().enumerate() {
                    match component {
                        Component::Prefix(_) | Component::ParentDir => {
                            let error = js_sys::Error::new("Failed to parse path");
                            error.set_cause(&JsValue::from_str("UnsupportedPath"));
                            return Err(error);
                        }
                        Component::RootDir | Component::CurDir => {
                            continue;
                        }
                        Component::Normal(segment) => {
                            let segment = segment.to_str().unwrap();

                            if i == components.len() - 1 {
                                let bytes =
                                    dir.get_file_handle(segment).await?.read_bytes().await?;

                                let mut cursor = std::io::Cursor::new(bytes);
                                return T::from_reader(&mut cursor);
                            } else {
                                dir = dir.get_dir_handle(segment).await?;
                            }
                        }
                    }
                }

                let error = js_sys::Error::new("Failed to parse path");
                error.set_cause(&JsValue::from_str("UnsupportedPath"));
                Err(error)
            }
            Source::URL(_) => {
                todo!()
            }
        }
    }
}
