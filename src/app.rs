use std::time::Instant;

use anyhow::{Result, anyhow};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::json;

use crate::{
    api::MihomoClient,
    local::{LocalStore, ProfileItem, Profiles},
    model::{Connection, Provider, ProxyGroup, Rule, Snapshot},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tab {
    Home,
    Proxies,
    Connections,
    Rules,
    Providers,
    Profiles,
    Logs,
    Settings,
}

impl Tab {
    pub const ALL: [Self; 8] = [
        Self::Home,
        Self::Proxies,
        Self::Connections,
        Self::Rules,
        Self::Providers,
        Self::Profiles,
        Self::Logs,
        Self::Settings,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Proxies => "Proxies",
            Self::Connections => "Connections",
            Self::Rules => "Rules",
            Self::Providers => "Providers",
            Self::Profiles => "Profiles",
            Self::Logs => "Logs",
            Self::Settings => "Settings",
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0)
    }
}

#[derive(Clone, Debug)]
pub enum InputMode {
    Search,
    ImportProfile,
}

#[derive(Clone, Debug)]
pub enum ConfirmAction {
    CloseAllConnections,
    DeleteProfile { uid: String, name: String },
    MarkProfile { uid: String, name: String },
    SystemProxy(bool),
    FlushDns,
    FlushFakeIp,
    UpdateGeo,
    UpgradeCore,
    RestartCore,
}

impl ConfirmAction {
    pub fn prompt(&self) -> String {
        match self {
            Self::CloseAllConnections => "Close every active connection?".into(),
            Self::DeleteProfile { name, .. } => format!("Delete profile '{name}' and its file?"),
            Self::MarkProfile { name, .. } => format!(
                "Mark '{name}' current? Clash Verge must enhance/reload it before runtime changes."
            ),
            Self::SystemProxy(enabled) => format!(
                "{} the desktop system proxy?",
                if *enabled { "Enable" } else { "Disable" }
            ),
            Self::FlushDns => "Flush Mihomo's DNS cache?".into(),
            Self::FlushFakeIp => "Flush Mihomo's fake-IP cache?".into(),
            Self::UpdateGeo => "Download/update Mihomo GEO data now?".into(),
            Self::UpgradeCore => "Ask Mihomo to upgrade its core now?".into(),
            Self::RestartCore => "Restart the Mihomo core now? Connections will drop.".into(),
        }
    }
}

pub struct App {
    pub client: MihomoClient,
    pub local: Option<LocalStore>,
    pub snapshot: Snapshot,
    pub profiles: Profiles,
    pub logs: Vec<String>,
    pub tab: Tab,
    pub selected: [usize; 8],
    pub proxy_group: Option<String>,
    pub proxy_provider_view: bool,
    pub query: String,
    pub input: String,
    pub input_mode: Option<InputMode>,
    pub confirm: Option<ConfirmAction>,
    pub help: bool,
    pub status: String,
    pub error: Option<String>,
    pub quit: bool,
    pub system_proxy: Option<bool>,
    pub last_refresh: Instant,
    full_redraw: bool,
}

impl App {
    pub fn new(client: MihomoClient, local: Option<LocalStore>) -> Self {
        Self {
            client,
            local,
            snapshot: Snapshot::default(),
            profiles: Profiles::default(),
            logs: Vec::new(),
            tab: Tab::Home,
            selected: [0; 8],
            proxy_group: None,
            proxy_provider_view: true,
            query: String::new(),
            input: String::new(),
            input_mode: None,
            confirm: None,
            help: false,
            status: "Starting…".into(),
            error: None,
            quit: false,
            system_proxy: None,
            last_refresh: Instant::now(),
            // A newly entered alternate screen is not guaranteed to match Ratatui's empty
            // previous buffer on every terminal emulator. Force the first frame to synchronize it.
            full_redraw: true,
        }
    }

    pub async fn refresh(&mut self) {
        let (version, configs, proxies, connections, rules, pp, rp) = tokio::join!(
            self.client.get("/version"),
            self.client.get("/configs"),
            self.client.get("/proxies"),
            self.client.get("/connections"),
            self.client.get("/rules"),
            self.client.get("/providers/proxies"),
            self.client.get("/providers/rules"),
        );
        let normalize = |result: anyhow::Result<_>| result.map_err(|error| error.to_string());
        let (snapshot, mut errors) = Snapshot::parse(
            normalize(version),
            normalize(configs),
            normalize(proxies),
            normalize(connections),
            normalize(rules),
            normalize(pp),
            normalize(rp),
        );
        self.snapshot = snapshot;
        if let Some(local) = &self.local {
            match local.load_profiles() {
                Ok(profiles) => self.profiles = profiles,
                Err(error) => errors.push(format!("local profiles: {error}")),
            }
            match local.read_logs(1500) {
                Ok(logs) => self.logs = logs,
                Err(error) => errors.push(format!("local logs: {error}")),
            }
            self.system_proxy = local.system_proxy_enabled();
        }
        self.clamp_selection();
        self.last_refresh = Instant::now();
        if errors.is_empty() {
            self.status = format!(
                "Updated {} · {} groups · {} connections",
                chrono::Local::now().format("%H:%M:%S"),
                self.snapshot.proxies.len(),
                self.snapshot.connections.len()
            );
            self.error = None;
        } else {
            self.error = Some(format!(
                "{} API request(s) failed: {}",
                errors.len(),
                errors[0]
            ));
        }
    }

    pub async fn handle_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        if self.help {
            self.help = false;
            return;
        }
        if self.input_mode.is_some() {
            self.handle_input(key).await;
            return;
        }
        if self.confirm.is_some() {
            self.handle_confirm(key).await;
            return;
        }

        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.help = true,
            KeyCode::Char('/') => {
                self.input_mode = Some(InputMode::Search);
                self.input = self.query.clone();
            }
            KeyCode::Char('r') => self.refresh().await,
            KeyCode::Tab | KeyCode::Right => self.next_tab(),
            KeyCode::BackTab | KeyCode::Left => self.previous_tab(),
            KeyCode::Char(value @ '1'..='8') => {
                let next = Tab::ALL[(value as usize) - ('1' as usize)];
                if self.tab != next {
                    self.tab = next;
                    self.proxy_group = None;
                    self.request_full_redraw();
                }
            }
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Home => self.set_selection(0),
            KeyCode::Char('g') if self.current_len() > 0 => self.set_selection(0),
            KeyCode::End => {
                let len = self.current_len();
                self.set_selection(len.saturating_sub(1));
            }
            KeyCode::Char('G') if self.current_len() > 0 => {
                let len = self.current_len();
                self.set_selection(len.saturating_sub(1));
            }
            KeyCode::Esc if self.proxy_group.is_some() => {
                self.proxy_group = None;
                self.set_selection(0);
                self.request_full_redraw();
            }
            code => self.handle_tab_key(code).await,
        }
    }

    async fn handle_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.input.clear();
                self.input_mode = None;
            }
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Char(value) => self.input.push(value),
            KeyCode::Enter => {
                let mode = self.input_mode.take();
                let value = std::mem::take(&mut self.input);
                match mode {
                    Some(InputMode::Search) => {
                        self.query = value;
                        self.set_selection(0);
                    }
                    Some(InputMode::ImportProfile) => {
                        let result = match &self.local {
                            Some(local) => local.import_profile(value.trim()).await.map(|uid| {
                                format!("Imported profile {uid}; select it and press Enter to mark current")
                            }),
                            None => Err(anyhow!("profile management requires a local Clash Verge data directory")),
                        };
                        self.finish_action(result);
                        self.refresh().await;
                    }
                    None => {}
                }
            }
            _ => {}
        }
    }

    async fn handle_confirm(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                if let Some(action) = self.confirm.take() {
                    let result = self.execute_confirmed(action).await;
                    self.finish_action(result);
                    self.refresh().await;
                }
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                self.confirm = None;
                self.status = "Cancelled".into();
            }
            _ => {}
        }
    }

    async fn execute_confirmed(&self, action: ConfirmAction) -> Result<String> {
        match action {
            ConfirmAction::CloseAllConnections => {
                self.client.delete("/connections").await?;
                Ok("Closed all connections".into())
            }
            ConfirmAction::DeleteProfile { uid, .. } => {
                self.local_ref()?.delete_profile(&uid)?;
                Ok("Deleted profile".into())
            }
            ConfirmAction::MarkProfile { uid, .. } => {
                self.local_ref()?.mark_profile_current(&uid)?;
                Ok(
                    "Saved current profile; reload/restart Clash Verge to enhance and apply it"
                        .into(),
                )
            }
            ConfirmAction::SystemProxy(enabled) => {
                self.local_ref()?.set_system_proxy(enabled)?;
                Ok(format!(
                    "System proxy {}",
                    if enabled { "enabled" } else { "disabled" }
                ))
            }
            ConfirmAction::FlushDns => {
                self.client.post("/cache/dns/flush", None).await?;
                Ok("Flushed DNS cache".into())
            }
            ConfirmAction::FlushFakeIp => {
                self.client.post("/cache/fakeip/flush", None).await?;
                Ok("Flushed fake-IP cache".into())
            }
            ConfirmAction::UpdateGeo => {
                self.client.post("/upgrade/geo", None).await?;
                Ok("Updated GEO data".into())
            }
            ConfirmAction::UpgradeCore => {
                self.client.post("/upgrade", None).await?;
                Ok("Core upgrade requested".into())
            }
            ConfirmAction::RestartCore => {
                self.client
                    .post("/restart", Some(json!({"path":"","payload":""})))
                    .await?;
                Ok("Core restart requested".into())
            }
        }
    }

    async fn handle_tab_key(&mut self, code: KeyCode) {
        let result = match self.tab {
            Tab::Home => self.home_key(code).await,
            Tab::Proxies => self.proxy_key(code).await,
            Tab::Connections => self.connection_key(code).await,
            Tab::Rules => self.rule_key(code).await,
            Tab::Providers => self.provider_key(code).await,
            Tab::Profiles => self.profile_key(code).await,
            Tab::Logs => Ok(None),
            Tab::Settings => self.settings_key(code).await,
        };
        match result {
            Ok(Some(message)) => {
                self.status = message;
                self.error = None;
                self.refresh().await;
            }
            Ok(None) => {}
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    async fn home_key(&mut self, code: KeyCode) -> Result<Option<String>> {
        match code {
            KeyCode::Char('m') => {
                let mode = match self.snapshot.mode.to_ascii_lowercase().as_str() {
                    "rule" => "global",
                    "global" => "direct",
                    _ => "rule",
                };
                self.client.patch_config("mode", json!(mode)).await?;
                Ok(Some(format!("Mode changed to {mode}")))
            }
            KeyCode::Char('t') => {
                let enabled = !self.snapshot.tun;
                self.client
                    .patch_config("tun", json!({"enable": enabled}))
                    .await?;
                Ok(Some(format!(
                    "TUN {}",
                    if enabled { "enabled" } else { "disabled" }
                )))
            }
            KeyCode::Char('s') => {
                self.confirm = Some(ConfirmAction::SystemProxy(
                    !self.system_proxy.unwrap_or(false),
                ));
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    async fn proxy_key(&mut self, code: KeyCode) -> Result<Option<String>> {
        match code {
            KeyCode::Enter if self.proxy_group.is_none() => {
                if let Some(group) = self.selected_group() {
                    self.proxy_group = Some(group.name.clone());
                    self.set_selection(0);
                    self.request_full_redraw();
                }
                Ok(None)
            }
            KeyCode::Enter => {
                let group = self
                    .proxy_group
                    .clone()
                    .ok_or_else(|| anyhow!("no group selected"))?;
                let proxy = self
                    .selected_proxy()
                    .ok_or_else(|| anyhow!("no proxy selected"))?;
                self.client.select_proxy(&group, &proxy).await?;
                Ok(Some(format!("Selected {proxy} for {group}")))
            }
            KeyCode::Char('d') => {
                let group = self
                    .proxy_group
                    .clone()
                    .or_else(|| self.selected_group().map(|group| group.name.clone()))
                    .ok_or_else(|| anyhow!("no group selected"))?;
                self.client.test_group(&group).await?;
                Ok(Some(format!("Latency test completed for {group}")))
            }
            _ => Ok(None),
        }
    }

    async fn connection_key(&mut self, code: KeyCode) -> Result<Option<String>> {
        match code {
            KeyCode::Char('x') | KeyCode::Delete => {
                let id = self
                    .selected_connection()
                    .map(|connection| connection.id.clone())
                    .ok_or_else(|| anyhow!("no connection selected"))?;
                self.client.close_connection(&id).await?;
                Ok(Some("Closed connection".into()))
            }
            KeyCode::Char('X') => {
                self.confirm = Some(ConfirmAction::CloseAllConnections);
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    async fn rule_key(&mut self, code: KeyCode) -> Result<Option<String>> {
        if matches!(code, KeyCode::Char('x') | KeyCode::Char(' ')) {
            let rule = self
                .selected_rule()
                .ok_or_else(|| anyhow!("no rule selected"))?;
            let (index, disabled) = (rule.index, !rule.disabled);
            self.client.set_rule_disabled(index, disabled).await?;
            return Ok(Some(format!(
                "Rule {index} {} (runtime only)",
                if disabled { "disabled" } else { "enabled" }
            )));
        }
        Ok(None)
    }

    async fn provider_key(&mut self, code: KeyCode) -> Result<Option<String>> {
        match code {
            KeyCode::Char('p') => {
                self.proxy_provider_view = !self.proxy_provider_view;
                self.set_selection(0);
                Ok(None)
            }
            KeyCode::Char('u') => {
                let name = self
                    .selected_provider()
                    .map(|provider| provider.name.clone())
                    .ok_or_else(|| anyhow!("no provider selected"))?;
                if self.proxy_provider_view {
                    self.client.update_proxy_provider(&name).await?;
                } else {
                    self.client.update_rule_provider(&name).await?;
                }
                Ok(Some(format!("Updated provider {name}")))
            }
            KeyCode::Char('h') if self.proxy_provider_view => {
                let name = self
                    .selected_provider()
                    .map(|provider| provider.name.clone())
                    .ok_or_else(|| anyhow!("no provider selected"))?;
                self.client.healthcheck_proxy_provider(&name).await?;
                Ok(Some(format!("Health checked provider {name}")))
            }
            _ => Ok(None),
        }
    }

    async fn profile_key(&mut self, code: KeyCode) -> Result<Option<String>> {
        match code {
            KeyCode::Char('a') => {
                self.input_mode = Some(InputMode::ImportProfile);
                self.input.clear();
                Ok(None)
            }
            KeyCode::Enter => {
                if let Some(item) = self.selected_profile() {
                    self.confirm = Some(ConfirmAction::MarkProfile {
                        uid: item.uid.clone(),
                        name: item.name.clone(),
                    });
                }
                Ok(None)
            }
            KeyCode::Char('u') => {
                let uid = self
                    .selected_profile()
                    .map(|item| item.uid.clone())
                    .ok_or_else(|| anyhow!("no profile selected"))?;
                self.local_ref()?.update_profile(&uid).await?;
                Ok(Some(
                    "Downloaded and atomically replaced subscription profile".into(),
                ))
            }
            KeyCode::Char('x') | KeyCode::Delete => {
                if let Some(item) = self.selected_profile() {
                    self.confirm = Some(ConfirmAction::DeleteProfile {
                        uid: item.uid.clone(),
                        name: item.name.clone(),
                    });
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    async fn settings_key(&mut self, code: KeyCode) -> Result<Option<String>> {
        match code {
            KeyCode::Char('d') => self.confirm = Some(ConfirmAction::FlushDns),
            KeyCode::Char('f') => self.confirm = Some(ConfirmAction::FlushFakeIp),
            KeyCode::Char('g') => self.confirm = Some(ConfirmAction::UpdateGeo),
            KeyCode::Char('u') => self.confirm = Some(ConfirmAction::UpgradeCore),
            KeyCode::Char('R') => self.confirm = Some(ConfirmAction::RestartCore),
            KeyCode::Char('l') => {
                let enabled = !self.snapshot.allow_lan;
                self.client
                    .patch_config("allow-lan", json!(enabled))
                    .await?;
                return Ok(Some(format!("Allow LAN set to {enabled}")));
            }
            KeyCode::Char('v') => {
                let enabled = !self.snapshot.ipv6;
                self.client.patch_config("ipv6", json!(enabled)).await?;
                return Ok(Some(format!("IPv6 set to {enabled}")));
            }
            _ => {}
        }
        Ok(None)
    }

    fn finish_action(&mut self, result: Result<String>) {
        match result {
            Ok(message) => {
                self.status = message;
                self.error = None;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn local_ref(&self) -> Result<&LocalStore> {
        self.local
            .as_ref()
            .ok_or_else(|| anyhow!("this action requires a local Clash Verge data directory"))
    }

    fn next_tab(&mut self) {
        self.tab = Tab::ALL[(self.tab.index() + 1) % Tab::ALL.len()];
        self.proxy_group = None;
        self.request_full_redraw();
    }

    fn previous_tab(&mut self) {
        self.tab = Tab::ALL[(self.tab.index() + Tab::ALL.len() - 1) % Tab::ALL.len()];
        self.proxy_group = None;
        self.request_full_redraw();
    }

    pub fn take_full_redraw(&mut self) -> bool {
        std::mem::take(&mut self.full_redraw)
    }

    fn request_full_redraw(&mut self) {
        self.full_redraw = true;
    }

    fn current_selection(&self) -> usize {
        self.selected[self.tab.index()]
    }

    fn set_selection(&mut self, value: usize) {
        self.selected[self.tab.index()] = value;
        self.clamp_selection();
    }

    fn move_selection(&mut self, delta: isize) {
        let len = self.current_len();
        if len == 0 {
            self.set_selection(0);
            return;
        }
        let current = self.current_selection() as isize;
        self.set_selection((current + delta).clamp(0, len as isize - 1) as usize);
    }

    fn clamp_selection(&mut self) {
        let len = self.current_len();
        let index = self.tab.index();
        self.selected[index] = self.selected[index].min(len.saturating_sub(1));
    }

    fn current_len(&self) -> usize {
        match self.tab {
            Tab::Home | Tab::Settings => 0,
            Tab::Proxies if self.proxy_group.is_some() => self.proxy_nodes().len(),
            Tab::Proxies => self.filtered_groups().len(),
            Tab::Connections => self.filtered_connections().len(),
            Tab::Rules => self.filtered_rules().len(),
            Tab::Providers => self.filtered_providers().len(),
            Tab::Profiles => self.filtered_profiles().len(),
            Tab::Logs => self.filtered_logs().len(),
        }
    }

    pub fn filtered_groups(&self) -> Vec<&ProxyGroup> {
        self.snapshot
            .proxies
            .iter()
            .filter(|group| matches_query(&self.query, &[&group.name, &group.now, &group.kind]))
            .collect()
    }

    pub fn proxy_nodes(&self) -> Vec<String> {
        self.proxy_group
            .as_deref()
            .and_then(|name| {
                self.snapshot
                    .proxies
                    .iter()
                    .find(|group| group.name == name)
            })
            .map(|group| {
                group
                    .all
                    .iter()
                    .filter(|name| matches_query(&self.query, &[name]))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn filtered_connections(&self) -> Vec<&Connection> {
        self.snapshot
            .connections
            .iter()
            .filter(|item| {
                matches_query(
                    &self.query,
                    &[&item.host, &item.destination, &item.process, &item.chains],
                )
            })
            .collect()
    }

    pub fn filtered_rules(&self) -> Vec<&Rule> {
        self.snapshot
            .rules
            .iter()
            .filter(|item| matches_query(&self.query, &[&item.kind, &item.payload, &item.proxy]))
            .collect()
    }

    pub fn filtered_providers(&self) -> Vec<&Provider> {
        let providers = if self.proxy_provider_view {
            &self.snapshot.proxy_providers
        } else {
            &self.snapshot.rule_providers
        };
        providers
            .iter()
            .filter(|item| matches_query(&self.query, &[&item.name, &item.kind, &item.vehicle]))
            .collect()
    }

    pub fn filtered_profiles(&self) -> Vec<&ProfileItem> {
        self.profiles
            .items
            .iter()
            .filter(|item| matches_query(&self.query, &[&item.name, &item.kind, &item.uid]))
            .collect()
    }

    pub fn filtered_logs(&self) -> Vec<&String> {
        self.logs
            .iter()
            .filter(|line| matches_query(&self.query, &[line]))
            .collect()
    }

    fn selected_group(&self) -> Option<&ProxyGroup> {
        self.filtered_groups()
            .get(self.current_selection())
            .copied()
    }

    fn selected_proxy(&self) -> Option<String> {
        self.proxy_nodes().get(self.current_selection()).cloned()
    }

    fn selected_connection(&self) -> Option<&Connection> {
        self.filtered_connections()
            .get(self.current_selection())
            .copied()
    }

    fn selected_rule(&self) -> Option<&Rule> {
        self.filtered_rules().get(self.current_selection()).copied()
    }

    fn selected_provider(&self) -> Option<&Provider> {
        self.filtered_providers()
            .get(self.current_selection())
            .copied()
    }

    fn selected_profile(&self) -> Option<&ProfileItem> {
        self.filtered_profiles()
            .get(self.current_selection())
            .copied()
    }
}

fn matches_query(query: &str, values: &[&str]) -> bool {
    if query.is_empty() {
        return true;
    }
    let query = query.to_lowercase();
    values
        .iter()
        .any(|value| value.to_lowercase().contains(&query))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crossterm::event::{KeyCode, KeyEvent};

    use super::{App, Tab, matches_query};
    use crate::{api::MihomoClient, config::Controller};

    #[test]
    fn search_is_case_insensitive() {
        assert!(matches_query("proxy", &["My PROXY"]));
        assert!(!matches_query("proxy", &["direct"]));
    }

    #[tokio::test]
    async fn tab_change_requests_exactly_one_full_redraw() {
        let client = MihomoClient::new(
            Controller::Unix(PathBuf::from("/tmp/nonexistent-test-socket")),
            None,
        )
        .unwrap();
        let mut app = App::new(client, None);
        assert!(
            app.take_full_redraw(),
            "the first frame must synchronize the terminal"
        );

        app.handle_key(KeyEvent::from(KeyCode::Tab)).await;
        assert_eq!(app.tab, Tab::Proxies);
        assert!(app.take_full_redraw());
        assert!(!app.take_full_redraw());

        app.handle_key(KeyEvent::from(KeyCode::Down)).await;
        assert!(!app.take_full_redraw());

        app.handle_key(KeyEvent::from(KeyCode::Char('3'))).await;
        assert_eq!(app.tab, Tab::Connections);
        assert!(app.take_full_redraw());
    }
}
