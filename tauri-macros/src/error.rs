use proc_macro::TokenStream;
use quote::quote;
use std::io::Error as IoError;
use std::path::PathBuf;
use Error::*;

pub(crate) enum Error {
  EnvOutDir,
  EnvCargoManifestDir,
  IncludeDirPrefix,
  Io(PathBuf, IoError),
}

/// Output a compiler error to the ast being transformed
impl From<Error> for TokenStream {
  fn from(error: Error) -> Self {
    let error: String = match error {
      EnvOutDir => "Unable to find OUT_DIR environmental variable from tauri-macros".into(),
      EnvCargoManifestDir => {
        "Unable to find CARGO_MANIFEST_DIR environmental variable from tauri-macros".into()
      }
      IncludeDirPrefix => "Invalid directory prefix encountered while including assets".into(),
      Io(path, error) => format!(
        "IO error {:?} encountered for {} during tauri-macros",
        error.kind(),
        path.display()
      ),
    };

    quote!(compile_error!(#error)).into()
  }
}
