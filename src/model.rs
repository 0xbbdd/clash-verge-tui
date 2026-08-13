use serde_json::Value;

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub online: bool,
    pub version: String,
    pub mode: String,
    pub tun: bool,
    pub allow_lan: bool,
    pub ipv6: bool,
    pub proxies: Vec<ProxyGroup>,
    pub connections: Vec<Connection>,
    pub rules: Vec<Rule>,
    pub proxy_providers: Vec<Provider>,
    pub rule_providers: Vec<Provider>,
    pub total_upload: u64,
    pub total_download: u64,
}

#[derive(Clone, Debug, Default)]
pub struct ProxyGroup {
    pub name: String,
    pub kind: String,
    pub now: String,
    pub all: Vec<String>,
    pub delay: Option<u64>,
}

#[derive(Clone, Debug, Default)]
pub struct Connection {
    pub id: String,
    pub host: String,
    pub destination: String,
    pub network: String,
    pub process: String,
    pub chains: String,
    pub upload: u64,
    pub download: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Rule {
    pub index: usize,
    pub kind: String,
    pub payload: String,
    pub proxy: String,
    pub hits: u64,
    pub disabled: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Provider {
    pub name: String,
    pub kind: String,
    pub vehicle: String,
    pub updated_at: String,
    pub count: usize,
}

impl Snapshot {
    pub fn parse(
        version: Result<Value, String>,
        configs: Result<Value, String>,
        proxies: Result<Value, String>,
        connections: Result<Value, String>,
        rules: Result<Value, String>,
        proxy_providers: Result<Value, String>,
        rule_providers: Result<Value, String>,
    ) -> (Self, Vec<String>) {
        let mut snapshot = Self::default();
        let mut errors = Vec::new();

        match version {
            Ok(value) => {
                snapshot.online = true;
                snapshot.version = text(&value, "version");
            }
            Err(error) => errors.push(error),
        }
        match configs {
            Ok(value) => {
                snapshot.mode = text(&value, "mode");
                snapshot.allow_lan = boolean(&value, "allow-lan");
                snapshot.ipv6 = boolean(&value, "ipv6");
                snapshot.tun = value
                    .get("tun")
                    .and_then(|tun| tun.get("enable"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
            }
            Err(error) => errors.push(error),
        }
        match proxies {
            Ok(value) => snapshot.proxies = parse_proxies(&value),
            Err(error) => errors.push(error),
        }
        match connections {
            Ok(value) => {
                snapshot.total_upload = value
                    .get("uploadTotal")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                snapshot.total_download = value
                    .get("downloadTotal")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                snapshot.connections = parse_connections(&value);
            }
            Err(error) => errors.push(error),
        }
        match rules {
            Ok(value) => snapshot.rules = parse_rules(&value),
            Err(error) => errors.push(error),
        }
        match proxy_providers {
            Ok(value) => snapshot.proxy_providers = parse_providers(&value),
            Err(error) => errors.push(error),
        }
        match rule_providers {
            Ok(value) => snapshot.rule_providers = parse_providers(&value),
            Err(error) => errors.push(error),
        }
        (snapshot, errors)
    }
}

fn parse_proxies(value: &Value) -> Vec<ProxyGroup> {
    let Some(items) = value.get("proxies").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut groups: Vec<_> = items
        .iter()
        .filter_map(|(name, value)| {
            let all = value.get("all")?.as_array()?;
            if all.is_empty() {
                return None;
            }
            Some(ProxyGroup {
                name: name.clone(),
                kind: text(value, "type"),
                now: text(value, "now"),
                all: all
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect(),
                delay: last_delay(value),
            })
        })
        .collect();
    groups.sort_by(|a, b| a.name.cmp(&b.name));
    groups
}

fn parse_connections(value: &Value) -> Vec<Connection> {
    value
        .get("connections")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|item| {
            let metadata = item.get("metadata").unwrap_or(&Value::Null);
            let host = text(metadata, "host");
            let ip = text(metadata, "destinationIP");
            let port = metadata
                .get("destinationPort")
                .and_then(Value::as_str)
                .unwrap_or_default();
            Connection {
                id: text(item, "id"),
                host,
                destination: format!("{ip}:{port}"),
                network: text(metadata, "network"),
                process: text(metadata, "processPath"),
                chains: item
                    .get("chains")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" → "),
                upload: item.get("upload").and_then(Value::as_u64).unwrap_or(0),
                download: item.get("download").and_then(Value::as_u64).unwrap_or(0),
            }
        })
        .collect()
}

fn parse_rules(value: &Value) -> Vec<Rule> {
    value
        .get("rules")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(position, item)| Rule {
            index: item
                .get("index")
                .and_then(Value::as_u64)
                .map(|index| index as usize)
                .unwrap_or(position),
            kind: text(item, "type"),
            payload: text(item, "payload"),
            proxy: text(item, "proxy"),
            hits: item
                .get("extra")
                .and_then(|extra| extra.get("hitCount"))
                .and_then(Value::as_u64)
                .unwrap_or(0),
            disabled: item
                .get("extra")
                .and_then(|extra| extra.get("disabled"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
        .collect()
}

fn parse_providers(value: &Value) -> Vec<Provider> {
    let Some(items) = value.get("providers").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut providers: Vec<_> = items
        .iter()
        .map(|(name, item)| Provider {
            name: name.clone(),
            kind: text(item, "type"),
            vehicle: text(item, "vehicleType"),
            updated_at: text(item, "updatedAt"),
            count: item
                .get("proxies")
                .and_then(Value::as_array)
                .map(Vec::len)
                .or_else(|| {
                    item.get("ruleCount")
                        .and_then(Value::as_u64)
                        .map(|n| n as usize)
                })
                .unwrap_or(0),
        })
        .collect();
    providers.sort_by(|a, b| a.name.cmp(&b.name));
    providers
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn boolean(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn last_delay(value: &Value) -> Option<u64> {
    value
        .get("history")?
        .as_array()?
        .last()?
        .get("delay")?
        .as_u64()
        .filter(|delay| *delay > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_policy_groups() {
        let value = serde_json::json!({"proxies": {
            "node": {"type": "Shadowsocks", "history": []},
            "GLOBAL": {"type": "Selector", "now": "node", "all": ["node"]}
        }});
        let groups = parse_proxies(&value);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "GLOBAL");
        assert_eq!(groups[0].now, "node");
    }

    #[test]
    fn parses_connection_totals() {
        let (snapshot, errors) = Snapshot::parse(
            Ok(serde_json::json!({"version":"v1"})),
            Ok(serde_json::json!({"mode":"rule"})),
            Ok(serde_json::json!({"proxies":{}})),
            Ok(serde_json::json!({"uploadTotal":12,"downloadTotal":34,"connections":[]})),
            Ok(serde_json::json!({"rules":[]})),
            Ok(serde_json::json!({"providers":{}})),
            Ok(serde_json::json!({"providers":{}})),
        );
        assert!(errors.is_empty());
        assert_eq!(snapshot.total_download, 34);
    }
}
