use std::{path::PathBuf, time::Duration};

use anyhow::{Context, Result, anyhow, bail};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Method, Request, StatusCode, client::conn::http1, header};
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};
use tokio::{net::UnixStream, time::timeout};

use crate::config::Controller;

#[derive(Clone)]
pub struct MihomoClient {
    controller: Controller,
    secret: Option<String>,
    http: reqwest::Client,
}

impl MihomoClient {
    pub fn new(controller: Controller, secret: Option<String>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .user_agent(concat!("clash-verge-tui/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self {
            controller,
            secret,
            http,
        })
    }

    pub async fn get(&self, path: &str) -> Result<Value> {
        self.request(Method::GET, path, None).await
    }

    pub async fn patch(&self, path: &str, body: Value) -> Result<Value> {
        self.request(Method::PATCH, path, Some(body)).await
    }

    pub async fn put(&self, path: &str, body: Option<Value>) -> Result<Value> {
        self.request(Method::PUT, path, body).await
    }

    pub async fn post(&self, path: &str, body: Option<Value>) -> Result<Value> {
        self.request(Method::POST, path, body).await
    }

    pub async fn delete(&self, path: &str) -> Result<Value> {
        self.request(Method::DELETE, path, None).await
    }

    pub async fn select_proxy(&self, group: &str, proxy: &str) -> Result<()> {
        let path = format!("/proxies/{}", urlencoding::encode(group));
        self.put(&path, Some(json!({"name": proxy}))).await?;
        Ok(())
    }

    pub async fn test_group(&self, group: &str) -> Result<()> {
        let path = format!(
            "/group/{}/delay?url={}&timeout=5000",
            urlencoding::encode(group),
            urlencoding::encode("https://www.gstatic.com/generate_204")
        );
        self.get(&path).await?;
        Ok(())
    }

    pub async fn patch_config(&self, key: &str, value: Value) -> Result<()> {
        self.patch("/configs", json!({key: value})).await?;
        Ok(())
    }

    pub async fn close_connection(&self, id: &str) -> Result<()> {
        self.delete(&format!("/connections/{}", urlencoding::encode(id)))
            .await?;
        Ok(())
    }

    pub async fn set_rule_disabled(&self, index: usize, disabled: bool) -> Result<()> {
        self.patch("/rules/disable", json!({index.to_string(): disabled}))
            .await?;
        Ok(())
    }

    pub async fn update_proxy_provider(&self, name: &str) -> Result<()> {
        self.put(
            &format!("/providers/proxies/{}", urlencoding::encode(name)),
            None,
        )
        .await?;
        Ok(())
    }

    pub async fn healthcheck_proxy_provider(&self, name: &str) -> Result<()> {
        self.get(&format!(
            "/providers/proxies/{}/healthcheck",
            urlencoding::encode(name)
        ))
        .await?;
        Ok(())
    }

    pub async fn update_rule_provider(&self, name: &str) -> Result<()> {
        self.put(
            &format!("/providers/rules/{}", urlencoding::encode(name)),
            None,
        )
        .await?;
        Ok(())
    }

    async fn request(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value> {
        match &self.controller {
            Controller::Unix(socket) => self.request_unix(socket, method, path, body).await,
            Controller::Http(base) => self.request_http(base, method, path, body).await,
        }
    }

    async fn request_http(
        &self,
        base: &str,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value> {
        let mut request = self.http.request(method, format!("{base}{path}"));
        if let Some(secret) = &self.secret {
            request = request.bearer_auth(secret);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.context("contact Mihomo controller")?;
        let status = response.status();
        let bytes = response.bytes().await.context("read Mihomo response")?;
        parse_response(status, &bytes)
    }

    async fn request_unix(
        &self,
        socket: &PathBuf,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value> {
        let stream = timeout(Duration::from_secs(3), UnixStream::connect(socket))
            .await
            .context("Mihomo Unix socket connection timed out")?
            .with_context(|| format!("connect {}", socket.display()))?;
        let (mut sender, connection) = http1::handshake(TokioIo::new(stream))
            .await
            .context("start HTTP over Mihomo Unix socket")?;
        tokio::spawn(async move {
            let _ = connection.await;
        });

        let bytes = body
            .map(|value| serde_json::to_vec(&value))
            .transpose()?
            .unwrap_or_default();
        let mut builder = Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, "localhost")
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(secret) = &self.secret {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {secret}"));
        }
        let request = builder.body(Full::new(Bytes::from(bytes)))?;
        let response = timeout(Duration::from_secs(8), sender.send_request(request))
            .await
            .context("Mihomo Unix request timed out")??;
        let status = response.status();
        let bytes = timeout(Duration::from_secs(8), response.into_body().collect())
            .await
            .context("Mihomo Unix response timed out")??
            .to_bytes();
        parse_response(status, &bytes)
    }
}

fn parse_response(status: StatusCode, bytes: &[u8]) -> Result<Value> {
    if !status.is_success() {
        let message = String::from_utf8_lossy(bytes);
        bail!("Mihomo returned {status}: {message}");
    }
    if bytes.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_slice(bytes).map_err(|error| {
        let preview = String::from_utf8_lossy(&bytes[..bytes.len().min(160)]);
        anyhow!("invalid JSON from Mihomo: {error}; response: {preview}")
    })
}
