use std::{fs, path::PathBuf, time::Duration};

use anyhow::{Context, Result, bail};
use clap::Parser;
use serde_yaml::Value;

const APP_DIR: &str = "io.github.clash-verge-rev.clash-verge-rev";

#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Args {
    /// Mihomo HTTP(S) controller, for example http://127.0.0.1:9090
    #[arg(long, env = "CVT_CONTROLLER")]
    pub controller: Option<String>,

    /// Mihomo Unix controller socket (preferred when Clash Verge Rev is local)
    #[arg(long, env = "CVT_SOCKET")]
    pub socket: Option<PathBuf>,

    /// Controller secret. If omitted, it is read from clash-verge.yaml/config.yaml
    #[arg(long, env = "CVT_SECRET")]
    pub secret: Option<String>,

    /// Clash Verge Rev data directory
    #[arg(long, env = "CVT_DATA_DIR")]
    pub data_dir: Option<PathBuf>,

    /// Automatic refresh interval in milliseconds
    #[arg(long, default_value_t = 2000, value_parser = clap::value_parser!(u64).range(250..))]
    pub interval: u64,

    /// Print detected paths and controller, then exit. Secrets are never printed.
    #[arg(long)]
    pub doctor: bool,
}

#[derive(Clone, Debug)]
pub enum Controller {
    Unix(PathBuf),
    Http(String),
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub controller: Controller,
    pub secret: Option<String>,
    pub data_dir: Option<PathBuf>,
    pub refresh_interval: Duration,
}

impl Settings {
    pub fn discover(args: Args) -> Result<Self> {
        let data_dir = args.data_dir.or_else(discover_data_dir);
        let documents = data_dir
            .as_ref()
            .map(|dir| [dir.join("clash-verge.yaml"), dir.join("config.yaml")])
            .unwrap_or_default();

        let file_socket = documents
            .iter()
            .filter_map(|path| read_yaml(path).ok())
            .find_map(|doc| string_at(&doc, "external-controller-unix").map(PathBuf::from));
        let file_controller = documents
            .iter()
            .filter_map(|path| read_yaml(path).ok())
            .find_map(|doc| string_at(&doc, "external-controller"))
            .filter(|value| !value.is_empty())
            .map(normalize_controller);
        let file_secret = documents
            .iter()
            .filter_map(|path| read_yaml(path).ok())
            .find_map(|doc| string_at(&doc, "secret"))
            .filter(|value| !value.is_empty());

        let controller = if let Some(path) = args.socket {
            Controller::Unix(path)
        } else if let Some(url) = args.controller {
            Controller::Http(normalize_controller(url))
        } else if let Some(path) = file_socket.filter(|path| path.exists()) {
            Controller::Unix(path)
        } else if let Some(url) = file_controller {
            Controller::Http(url)
        } else {
            let default_socket = PathBuf::from("/tmp/verge/verge-mihomo.sock");
            if default_socket.exists() {
                Controller::Unix(default_socket)
            } else {
                bail!(
                    "no Mihomo controller found; pass --socket or --controller, or start Clash Verge Rev"
                );
            }
        };

        Ok(Self {
            controller,
            secret: args.secret.or(file_secret),
            data_dir,
            refresh_interval: Duration::from_millis(args.interval),
        })
    }

    pub fn doctor(&self) -> String {
        let controller = match &self.controller {
            Controller::Unix(path) => format!("unix:{}", path.display()),
            Controller::Http(url) => url.clone(),
        };
        format!(
            "controller: {controller}\ndata-dir: {}\nauth: {}\nrefresh: {}ms",
            self.data_dir
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "not found (remote/core-only mode)".into()),
            if self.secret.is_some() {
                "configured"
            } else {
                "none"
            },
            self.refresh_interval.as_millis()
        )
    }
}

fn discover_data_dir() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let mut candidates = vec![
        home.join(".local/share").join(APP_DIR),
        home.join("Library/Application Support").join(APP_DIR),
    ];
    if let Some(data_dir) = dirs::data_dir() {
        candidates.push(data_dir.join(APP_DIR));
    }
    candidates.into_iter().find(|path| path.is_dir())
}

fn read_yaml(path: &PathBuf) -> Result<Value> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_yaml::from_str(&text).with_context(|| format!("parse {}", path.display()))
}

fn string_at(value: &Value, key: &str) -> Option<String> {
    value
        .as_mapping()?
        .get(Value::String(key.into()))?
        .as_str()
        .map(ToOwned::to_owned)
}

fn normalize_controller(value: String) -> String {
    let value = value.trim_end_matches('/');
    if value.starts_with("http://") || value.starts_with("https://") {
        value.into()
    } else {
        format!("http://{value}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_normalization_preserves_scheme() {
        assert_eq!(
            normalize_controller("127.0.0.1:9090/".into()),
            "http://127.0.0.1:9090"
        );
        assert_eq!(
            normalize_controller("https://host/api/".into()),
            "https://host/api"
        );
    }
}
