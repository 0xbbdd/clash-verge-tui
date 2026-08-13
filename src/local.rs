use std::{
    fs::{self, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};

const MAX_LOG_TAIL_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ProfileItem {
    #[serde(default)]
    pub uid: String,
    #[serde(default, rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub updated: Option<u64>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct ProfilesFile {
    #[serde(default)]
    current: Option<String>,
    #[serde(default)]
    items: Vec<ProfileItem>,
}

#[derive(Clone, Debug, Default)]
pub struct Profiles {
    pub current: Option<String>,
    pub items: Vec<ProfileItem>,
}

#[derive(Clone, Debug)]
pub struct LocalStore {
    data_dir: PathBuf,
    http: reqwest::Client,
}

impl LocalStore {
    pub fn new(data_dir: PathBuf) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent(concat!("clash-verge-tui/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self { data_dir, http })
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn load_profiles(&self) -> Result<Profiles> {
        let path = self.data_dir.join("profiles.yaml");
        let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let file: ProfilesFile =
            serde_yaml::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
        Ok(Profiles {
            current: file.current,
            items: file.items,
        })
    }

    /// Marks a profile current in Clash Verge's profile index. Clash Verge must enhance/reload it.
    pub fn mark_profile_current(&self, uid: &str) -> Result<()> {
        let path = self.data_dir.join("profiles.yaml");
        self.mutate_profiles(&path, |root| {
            let mapping = root
                .as_mapping_mut()
                .ok_or_else(|| anyhow!("profiles.yaml root is not a mapping"))?;
            let items = mapping
                .get(Value::String("items".into()))
                .and_then(Value::as_sequence)
                .ok_or_else(|| anyhow!("profiles.yaml has no items"))?;
            let exists = items
                .iter()
                .any(|item| string_field(item, "uid").as_deref() == Some(uid));
            if !exists {
                bail!("profile no longer exists");
            }
            mapping.insert(Value::String("current".into()), Value::String(uid.into()));
            Ok(())
        })
    }

    pub async fn import_profile(&self, url: &str) -> Result<String> {
        validate_subscription_url(url)?;
        let content = self.download_profile(url).await?;
        validate_profile_yaml(&content)?;

        let epoch = now_epoch()?;
        let uid = format!("tui{}", now_nanos()?);
        let filename = format!("{uid}.yaml");
        let profile_path = self.data_dir.join("profiles").join(&filename);
        atomic_write(&profile_path, &content)?;

        let index_path = self.data_dir.join("profiles.yaml");
        let result = self.mutate_profiles(&index_path, |root| {
            let mapping = root
                .as_mapping_mut()
                .ok_or_else(|| anyhow!("profiles.yaml root is not a mapping"))?;
            let items = mapping
                .get_mut(Value::String("items".into()))
                .and_then(Value::as_sequence_mut)
                .ok_or_else(|| anyhow!("profiles.yaml has no items list"))?;
            let mut item = Mapping::new();
            item.insert(Value::String("uid".into()), Value::String(uid.clone()));
            item.insert(Value::String("type".into()), Value::String("remote".into()));
            item.insert(
                Value::String("name".into()),
                Value::String(format!("TUI import {epoch}")),
            );
            item.insert(
                Value::String("file".into()),
                Value::String(filename.clone()),
            );
            item.insert(Value::String("url".into()), Value::String(url.into()));
            item.insert(Value::String("updated".into()), Value::Number(epoch.into()));
            items.push(Value::Mapping(item));
            Ok(())
        });
        if let Err(error) = result {
            let _ = fs::remove_file(&profile_path);
            return Err(error);
        }
        Ok(uid)
    }

    pub async fn update_profile(&self, uid: &str) -> Result<()> {
        let profiles = self.load_profiles()?;
        let item = profiles
            .items
            .iter()
            .find(|item| item.uid == uid)
            .ok_or_else(|| anyhow!("profile no longer exists"))?;
        let url = item
            .url
            .as_deref()
            .ok_or_else(|| anyhow!("profile has no remote URL"))?;
        validate_subscription_url(url)?;
        let content = self.download_profile(url).await?;
        validate_profile_yaml(&content)?;
        let target = safe_profile_path(&self.data_dir, &item.file)?;
        atomic_write(&target, &content)?;

        let index_path = self.data_dir.join("profiles.yaml");
        self.mutate_profiles(&index_path, |root| {
            let Some(items) = root
                .as_mapping_mut()
                .and_then(|map| map.get_mut(Value::String("items".into())))
                .and_then(Value::as_sequence_mut)
            else {
                bail!("profiles.yaml has no items list");
            };
            let Some(item) = items
                .iter_mut()
                .find(|item| string_field(item, "uid").as_deref() == Some(uid))
            else {
                bail!("profile disappeared while updating");
            };
            if let Some(map) = item.as_mapping_mut() {
                map.insert(
                    Value::String("updated".into()),
                    Value::Number(now_epoch()?.into()),
                );
            }
            Ok(())
        })
    }

    pub fn delete_profile(&self, uid: &str) -> Result<()> {
        let profiles = self.load_profiles()?;
        if profiles.current.as_deref() == Some(uid) {
            bail!("refusing to delete the current profile");
        }
        let item = profiles
            .items
            .iter()
            .find(|item| item.uid == uid)
            .ok_or_else(|| anyhow!("profile no longer exists"))?;
        let target = safe_profile_path(&self.data_dir, &item.file)?;
        let tombstone = target.with_file_name(format!(
            ".{}.deleted-{}-{}",
            target
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("profile"),
            std::process::id(),
            now_nanos()?
        ));
        let moved = if target.exists() {
            fs::rename(&target, &tombstone)
                .with_context(|| format!("stage deletion of {}", target.display()))?;
            true
        } else {
            false
        };
        let index_path = self.data_dir.join("profiles.yaml");
        let index_result = self.mutate_profiles(&index_path, |root| {
            let Some(items) = root
                .as_mapping_mut()
                .and_then(|map| map.get_mut(Value::String("items".into())))
                .and_then(Value::as_sequence_mut)
            else {
                bail!("profiles.yaml has no items list");
            };
            let old_len = items.len();
            items.retain(|item| string_field(item, "uid").as_deref() != Some(uid));
            if items.len() == old_len {
                bail!("profile disappeared while deleting");
            }
            Ok(())
        });
        if let Err(error) = index_result {
            if moved {
                fs::rename(&tombstone, &target).with_context(|| {
                    format!(
                        "restore {} after profile index update failed: {error}",
                        target.display()
                    )
                })?;
            }
            return Err(error);
        }
        if moved {
            fs::remove_file(&tombstone)
                .with_context(|| format!("remove staged profile {}", tombstone.display()))?;
        }
        Ok(())
    }

    pub fn read_logs(&self, max_lines: usize) -> Result<Vec<String>> {
        let candidates = [
            self.data_dir.join("logs/latest.log"),
            self.data_dir.join("logs/service/service_latest.log"),
            self.data_dir.join("logs/sidecar/sidecar_latest.log"),
        ];
        let mut logs = Vec::new();
        for path in candidates {
            let Ok(text) = read_file_tail(&path, MAX_LOG_TAIL_BYTES) else {
                continue;
            };
            let label = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("log");
            logs.extend(text.lines().map(|line| format!("[{label}] {line}")));
        }
        let skip = logs.len().saturating_sub(max_lines);
        Ok(logs.into_iter().skip(skip).collect())
    }

    pub fn system_proxy_enabled(&self) -> Option<bool> {
        #[cfg(target_os = "linux")]
        {
            let output = Command::new("gsettings")
                .args(["get", "org.gnome.system.proxy", "mode"])
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }
            let value = String::from_utf8_lossy(&output.stdout);
            return Some(value.contains("manual") || value.contains("auto"));
        }
        #[allow(unreachable_code)]
        None
    }

    pub fn set_system_proxy(&self, enabled: bool) -> Result<()> {
        #[cfg(target_os = "linux")]
        {
            let (host, port) = self.proxy_endpoint()?;
            if enabled {
                run_gsettings(&["set", "org.gnome.system.proxy.http", "host", &host])?;
                run_gsettings(&[
                    "set",
                    "org.gnome.system.proxy.http",
                    "port",
                    &port.to_string(),
                ])?;
                run_gsettings(&["set", "org.gnome.system.proxy.https", "host", &host])?;
                run_gsettings(&[
                    "set",
                    "org.gnome.system.proxy.https",
                    "port",
                    &port.to_string(),
                ])?;
                run_gsettings(&["set", "org.gnome.system.proxy", "mode", "manual"])?;
            } else {
                run_gsettings(&["set", "org.gnome.system.proxy", "mode", "none"])?;
            }
            self.patch_verge_bool("enable_system_proxy", enabled)?;
            Ok(())
        }
        #[cfg(not(target_os = "linux"))]
        bail!("system proxy control is currently implemented for Linux/GNOME only")
    }

    pub fn patch_verge_bool(&self, key: &str, enabled: bool) -> Result<()> {
        let path = self.data_dir.join("verge.yaml");
        let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let mut root: Value = serde_yaml::from_str(&text)?;
        let mapping = root
            .as_mapping_mut()
            .ok_or_else(|| anyhow!("verge.yaml root is not a mapping"))?;
        mapping.insert(Value::String(key.into()), Value::Bool(enabled));
        let output = serde_yaml::to_string(&root)?;
        atomic_write(&path, output.as_bytes())
    }

    async fn download_profile(&self, url: &str) -> Result<Vec<u8>> {
        let response = self
            .http
            .get(url)
            .send()
            .await
            .context("download subscription")?;
        if !response.status().is_success() {
            bail!("subscription server returned {}", response.status());
        }
        let bytes = response.bytes().await.context("read subscription")?;
        if bytes.len() > 16 * 1024 * 1024 {
            bail!("subscription is larger than the 16 MiB safety limit");
        }
        Ok(bytes.to_vec())
    }

    fn mutate_profiles<F>(&self, path: &Path, mutate: F) -> Result<()>
    where
        F: FnOnce(&mut Value) -> Result<()>,
    {
        let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        let mut root: Value =
            serde_yaml::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
        mutate(&mut root)?;
        let output = serde_yaml::to_string(&root)?;
        atomic_write(path, output.as_bytes())
    }

    fn proxy_endpoint(&self) -> Result<(String, u16)> {
        for filename in ["clash-verge.yaml", "config.yaml"] {
            let path = self.data_dir.join(filename);
            let Ok(text) = fs::read_to_string(path) else {
                continue;
            };
            let Ok(root) = serde_yaml::from_str::<Value>(&text) else {
                continue;
            };
            let Some(map) = root.as_mapping() else {
                continue;
            };
            let port = map
                .get(Value::String("mixed-port".into()))
                .and_then(Value::as_u64)
                .and_then(|port| u16::try_from(port).ok());
            if let Some(port) = port {
                return Ok(("127.0.0.1".into(), port));
            }
        }
        bail!("could not find mixed-port in Clash Verge configuration")
    }
}

fn safe_profile_path(data_dir: &Path, filename: &str) -> Result<PathBuf> {
    let filename = Path::new(filename);
    if filename.components().count() != 1 || filename.file_name().is_none() {
        bail!("unsafe profile filename");
    }
    Ok(data_dir.join("profiles").join(filename))
}

fn validate_subscription_url(url: &str) -> Result<()> {
    let parsed = reqwest::Url::parse(url).context("invalid subscription URL")?;
    if !matches!(parsed.scheme(), "http" | "https") {
        bail!("subscription URL must use http or https");
    }
    Ok(())
}

fn validate_profile_yaml(bytes: &[u8]) -> Result<()> {
    let value: Value = serde_yaml::from_slice(bytes).context("subscription is not valid YAML")?;
    let mapping = value
        .as_mapping()
        .ok_or_else(|| anyhow!("subscription YAML root is not a mapping"))?;
    let looks_like_profile = ["proxies", "proxy-providers", "proxy-groups", "rules"]
        .into_iter()
        .any(|key| mapping.contains_key(Value::String(key.into())));
    if !looks_like_profile {
        bail!("downloaded YAML does not look like a Mihomo profile");
    }
    Ok(())
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .as_mapping()?
        .get(Value::String(key.into()))?
        .as_str()
        .map(ToOwned::to_owned)
}

fn now_epoch() -> Result<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}

fn now_nanos() -> Result<u128> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos())
}

fn read_file_tail(path: &Path, max_bytes: u64) -> Result<String> {
    let mut file = fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(max_bytes);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::with_capacity((len - start) as usize);
    file.read_to_end(&mut bytes)?;
    if start > 0
        && let Some(newline) = bytes.iter().position(|byte| *byte == b'\n')
    {
        bytes.drain(..=newline);
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or_else(|| anyhow!("path has no parent"))?;
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    let temp = parent.join(format!(
        ".{}.cvt-{}-{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("config"),
        std::process::id(),
        now_epoch()?
    ));
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temp)
        .with_context(|| format!("create {}", temp.display()))?;
    if let Err(error) = (|| -> Result<()> {
        file.write_all(content)?;
        file.sync_all()?;
        fs::rename(&temp, path).with_context(|| format!("replace {}", path.display()))?;
        Ok(())
    })() {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn run_gsettings(args: &[&str]) -> Result<()> {
    let output = Command::new("gsettings")
        .args(args)
        .output()
        .context("run gsettings")?;
    if !output.status.success() {
        bail!(
            "gsettings failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_profile_path_traversal() {
        let dir = Path::new("/tmp/example");
        assert!(safe_profile_path(dir, "../secret").is_err());
        assert!(safe_profile_path(dir, "/absolute").is_err());
        assert_eq!(
            safe_profile_path(dir, "abc.yaml").unwrap(),
            dir.join("profiles/abc.yaml")
        );
    }

    #[test]
    fn profile_validation_requires_mihomo_keys() {
        assert!(validate_profile_yaml(b"hello: world").is_err());
        assert!(validate_profile_yaml(b"proxies: []").is_ok());
    }

    #[test]
    fn atomic_write_replaces_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("value.yaml");
        fs::write(&path, "old").unwrap();
        atomic_write(&path, b"new").unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), "new");
    }

    #[test]
    fn log_tail_drops_a_partial_first_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("latest.log");
        fs::write(&path, "first line\nsecond line\nthird line\n").unwrap();
        assert_eq!(read_file_tail(&path, 17).unwrap(), "third line\n");
        assert_eq!(
            read_file_tail(&path, 1024).unwrap(),
            fs::read_to_string(path).unwrap()
        );
    }
}
