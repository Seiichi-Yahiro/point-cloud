use crate::plugins::asset::Asset;
use std::error::Error;
use std::fmt::{Display, Formatter};
use url::Url;

#[derive(Debug, Clone)]
pub enum SourceError {
    NotFound(String),
    NoSource,
    #[cfg(target_arch = "wasm32")]
    InvalidPath(String),
    Other {
        message: String,
        #[cfg(not(target_arch = "wasm32"))]
        name: std::io::ErrorKind,
        #[cfg(target_arch = "wasm32")]
        name: String,
    },
}

impl From<std::io::Error> for SourceError {
    fn from(value: std::io::Error) -> Self {
        match value.kind() {
            std::io::ErrorKind::NotFound => Self::NotFound(value.to_string()),
            _ => Self::Other {
                message: value.to_string(),
                #[cfg(not(target_arch = "wasm32"))]
                name: value.kind(),
                #[cfg(target_arch = "wasm32")]
                name: value.kind().to_string(),
            },
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl From<wasm_bindgen::JsValue> for SourceError {
    fn from(value: wasm_bindgen::JsValue) -> Self {
        js_sys::Error::from(value).into()
    }
}

#[cfg(target_arch = "wasm32")]
impl From<js_sys::Error> for SourceError {
    fn from(value: js_sys::Error) -> Self {
        let name = value.name().as_string().unwrap();

        match name.as_str() {
            "NotFoundError" => Self::NotFound(value.message().as_string().unwrap()),
            _ => Self::Other {
                message: value.message().as_string().unwrap(),
                name,
            },
        }
    }
}

impl Display for SourceError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let msg = match self {
            SourceError::NotFound(msg) => msg,
            #[cfg(target_arch = "wasm32")]
            SourceError::InvalidPath(msg) => msg,
            SourceError::NoSource => "no source provided",
            SourceError::Other { message, .. } => message,
        };

        write!(f, "{}", msg)
    }
}
impl Error for SourceError {}

#[derive(Debug, Clone)]
pub enum Directory {
    #[cfg(not(target_arch = "wasm32"))]
    Path(std::path::PathBuf),
    #[cfg(target_arch = "wasm32")]
    WebDir(crate::web::WebDir),
    URL(Url),
}

impl Directory {
    pub fn join(&self, path: &std::path::Path) -> Source {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Directory::Path(dir) => Source::Path(dir.join(path)),
            #[cfg(target_arch = "wasm32")]
            Directory::WebDir(dir) => Source::PathInDirectory {
                directory: dir.clone(),
                path: path.to_path_buf(),
            },
            Directory::URL(url) => {
                let mut url = url.clone();
                url.set_path(path.to_str().unwrap());
                Source::URL(url)
            }
        }
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

    URL(Url),

    None,
}

impl Source {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load<T: Asset>(&self) -> Result<T, SourceError> {
        match self {
            Source::Path(path) => {
                let file = std::fs::File::open(path)?;
                let mut buf_reader = std::io::BufReader::new(file);
                T::read_from(&mut buf_reader)
            }
            Source::URL(url) => {
                let request = ehttp::Request::get(url);
                let response = ehttp::fetch_blocking(&request);
                handle_response_from_url(url, response)
            }
            Source::None => Err(SourceError::NoSource),
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn load<T: Asset>(&self) -> Result<T, SourceError> {
        match self {
            Source::PathInDirectory { directory, path } => {
                use std::path::Component;

                let mut dir = directory.clone();

                let components = path.components().collect::<Vec<_>>();

                for (i, component) in components.iter().enumerate() {
                    match component {
                        Component::Prefix(_) | Component::ParentDir => {
                            return Err(SourceError::InvalidPath(
                                path.to_str().unwrap().to_string(),
                            ));
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
                                return T::read_from(&mut cursor);
                            } else {
                                dir = dir.get_dir_handle(segment).await?;
                            }
                        }
                    }
                }

                Err(SourceError::InvalidPath(path.to_str().unwrap().to_string()))
            }
            Source::URL(url) => {
                let mut request = ehttp::Request::get(url);
                let response = ehttp::fetch_async(request).await;
                handle_response_from_url(url, response)
            }
            Source::None => Err(SourceError::NoSource),
        }
    }
}

fn handle_response_from_url<T: Asset>(
    url: &Url,
    response: ehttp::Result<ehttp::Response>,
) -> Result<T, SourceError> {
    match response {
        Ok(response) => {
            if (200..300).contains(&response.status) {
                let mut cursor = std::io::Cursor::new(response.bytes);
                T::read_from(&mut cursor)
            } else if response.status == 404 {
                Err(SourceError::NotFound(url.to_string()))
            } else {
                Err(SourceError::Other {
                    message: response.status_text,
                    #[cfg(not(target_arch = "wasm32"))]
                    name: std::io::ErrorKind::Other,
                    #[cfg(target_arch = "wasm32")]
                    name: "Unsupported HTTP Status".to_string(),
                })
            }
        }
        Err(err) => Err(SourceError::Other {
            message: err,
            #[cfg(not(target_arch = "wasm32"))]
            name: std::io::ErrorKind::Other,
            #[cfg(target_arch = "wasm32")]
            name: "Request failed".to_string(),
        }),
    }
}
