//! Actualizaciones deshabilitadas deliberadamente en esta variante local.
//! No se realizan solicitudes HTTP ni se descargan o ejecutan binarios externos.

use std::path::{Path, PathBuf};
use anyhow::{bail, Result};

#[derive(Default, Clone)]
pub struct State { pub url: Option<String> }

pub fn load() -> State { State::default() }
pub fn current() -> &'static str { env!("CARGO_PKG_VERSION") }
pub fn available() -> Option<(String, String)> { None }
pub fn notes(_version: &str) -> Option<String> { None }
pub fn news() -> Option<(String, String)> { None }
pub fn news_seen() {}
pub fn check_now() -> Result<Option<String>> { bail!("Actualizaciones remotas deshabilitadas") }
pub fn watch(_notify: impl Fn() + Send + 'static) {}
pub fn download(_url: &str) -> Result<PathBuf> { bail!("Descargas remotas deshabilitadas") }
pub fn start_install(_downloaded: &Path) -> Result<()> { bail!("Instalación remota deshabilitada") }
pub fn apply(_target: &Path) -> i32 { 1 }
pub fn cleanup() {}
