use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Table, TableState,
        Tabs, Wrap,
    },
};

use crate::app::{App, InputMode, Tab};

const ACCENT: Color = Color::Cyan;
const MUTED: Color = Color::DarkGray;

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(2),
        ])
        .split(area);

    draw_tabs(frame, app, layout[0]);
    match app.tab {
        Tab::Home => draw_home(frame, app, layout[1]),
        Tab::Proxies => draw_proxies(frame, app, layout[1]),
        Tab::Connections => draw_connections(frame, app, layout[1]),
        Tab::Rules => draw_rules(frame, app, layout[1]),
        Tab::Providers => draw_providers(frame, app, layout[1]),
        Tab::Profiles => draw_profiles(frame, app, layout[1]),
        Tab::Logs => draw_logs(frame, app, layout[1]),
        Tab::Settings => draw_settings(frame, app, layout[1]),
    }
    draw_footer(frame, app, layout[2]);

    if app.help {
        draw_help(frame, area);
    } else if let Some(action) = &app.confirm {
        draw_popup(
            frame,
            area,
            "Confirm",
            vec![
                Line::from(action.prompt()),
                Line::from(""),
                Line::from(vec![
                    Span::styled(
                        "Enter/y",
                        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" confirm   "),
                    Span::styled("n/Esc", Style::default().fg(ACCENT)),
                    Span::raw(" cancel"),
                ]),
            ],
            62,
            7,
        );
    } else if let Some(mode) = &app.input_mode {
        let title = match mode {
            InputMode::Search => "Search",
            InputMode::ImportProfile => "Import subscription URL",
        };
        draw_popup(
            frame,
            area,
            title,
            vec![
                Line::from(app.input.as_str()),
                Line::from(""),
                Line::styled("Enter apply · Esc cancel", Style::default().fg(MUTED)),
            ],
            72,
            6,
        );
        let popup = centered(area, 72, 6);
        frame.set_cursor_position((
            popup.x
                + 1
                + app
                    .input
                    .chars()
                    .count()
                    .min(popup.width.saturating_sub(3) as usize) as u16,
            popup.y + 1,
        ));
    }
}

fn draw_tabs(frame: &mut Frame, app: &App, area: Rect) {
    let titles = Tab::ALL
        .iter()
        .enumerate()
        .map(|(index, tab)| Line::from(format!(" {} {} ", index + 1, tab.title())))
        .collect::<Vec<_>>();
    let selected = Tab::ALL.iter().position(|tab| *tab == app.tab).unwrap_or(0);
    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .title(format!(" clash-verge-tui {} ", env!("CARGO_PKG_VERSION"))),
        )
        .select(selected)
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(ACCENT)
                .add_modifier(Modifier::BOLD),
        )
        .divider(Span::raw(""));
    frame.render_widget(tabs, area);
}

fn draw_home(frame: &mut Frame, app: &App, area: Rect) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(52), Constraint::Percentage(48)])
        .split(area);
    let status = if app.snapshot.online {
        "ONLINE"
    } else {
        "OFFLINE"
    };
    let system_proxy = app.system_proxy.map(on_off).unwrap_or("unavailable");
    let overview = vec![
        Line::from(vec![
            Span::raw("Core          "),
            Span::styled(status, bool_style(app.snapshot.online)),
        ]),
        Line::from(format!(
            "Version       {}",
            value_or_dash(&app.snapshot.version)
        )),
        Line::from(format!(
            "Mode          {}",
            value_or_dash(&app.snapshot.mode)
        )),
        Line::from(format!("TUN           {}", on_off(app.snapshot.tun))),
        Line::from(format!("System proxy  {system_proxy}")),
        Line::from(format!("Allow LAN     {}", on_off(app.snapshot.allow_lan))),
        Line::from(format!("IPv6          {}", on_off(app.snapshot.ipv6))),
    ];
    frame.render_widget(
        Paragraph::new(overview).block(Block::default().borders(Borders::ALL).title(" Runtime ")),
        columns[0],
    );

    let traffic = vec![
        Line::from(format!(
            "Active connections  {}",
            app.snapshot.connections.len()
        )),
        Line::from(format!(
            "Policy groups       {}",
            app.snapshot.proxies.len()
        )),
        Line::from(format!("Rules               {}", app.snapshot.rules.len())),
        Line::from(""),
        Line::from(format!(
            "Session upload      {}",
            human_bytes(app.snapshot.total_upload)
        )),
        Line::from(format!(
            "Session download    {}",
            human_bytes(app.snapshot.total_download)
        )),
    ];
    frame.render_widget(
        Paragraph::new(traffic).block(Block::default().borders(Borders::ALL).title(" Activity ")),
        columns[1],
    );
}

fn draw_proxies(frame: &mut Frame, app: &App, area: Rect) {
    if let Some(group_name) = &app.proxy_group {
        let selected_now = app
            .snapshot
            .proxies
            .iter()
            .find(|group| &group.name == group_name)
            .map(|group| group.now.as_str())
            .unwrap_or_default();
        let rows = app.proxy_nodes().into_iter().map(|node| {
            let marker = if node == selected_now { "●" } else { " " };
            Row::new([Cell::from(marker), Cell::from(node)])
        });
        render_table(
            frame,
            area,
            &format!(" {group_name} · Enter select · Esc groups "),
            ["", "Proxy"],
            rows,
            [Constraint::Length(2), Constraint::Min(20)],
            app.selected[app.tab_index()],
        );
    } else {
        let rows = app.filtered_groups().into_iter().map(|group| {
            Row::new([
                Cell::from(group.name.clone()),
                Cell::from(group.kind.clone()),
                Cell::from(group.now.clone()),
                Cell::from(
                    group
                        .delay
                        .map(|delay| format!("{delay} ms"))
                        .unwrap_or_else(|| "—".into()),
                ),
                Cell::from(group.all.len().to_string()),
            ])
        });
        render_table(
            frame,
            area,
            " Policy groups · Enter open · d latency test ",
            ["Group", "Type", "Selected", "Delay", "Nodes"],
            rows,
            [
                Constraint::Percentage(23),
                Constraint::Length(12),
                Constraint::Percentage(35),
                Constraint::Length(10),
                Constraint::Length(7),
            ],
            app.selected[app.tab_index()],
        );
    }
}

fn draw_connections(frame: &mut Frame, app: &App, area: Rect) {
    let rows = app.filtered_connections().into_iter().map(|item| {
        Row::new([
            Cell::from(if item.host.is_empty() {
                item.destination.clone()
            } else {
                item.host.clone()
            }),
            Cell::from(item.network.clone()),
            Cell::from(short_process(&item.process)),
            Cell::from(item.chains.clone()),
            Cell::from(human_bytes(item.upload)),
            Cell::from(human_bytes(item.download)),
        ])
    });
    render_table(
        frame,
        area,
        " Connections · x close selected · X close all ",
        ["Host", "Net", "Process", "Chain", "Up", "Down"],
        rows,
        [
            Constraint::Percentage(25),
            Constraint::Length(6),
            Constraint::Percentage(18),
            Constraint::Percentage(27),
            Constraint::Length(9),
            Constraint::Length(9),
        ],
        app.selected[app.tab_index()],
    );
}

fn draw_rules(frame: &mut Frame, app: &App, area: Rect) {
    let rows = app.filtered_rules().into_iter().map(|rule| {
        Row::new([
            Cell::from(rule.index.to_string()),
            Cell::from(if rule.disabled { "off" } else { "on" }),
            Cell::from(rule.kind.clone()),
            Cell::from(rule.payload.clone()),
            Cell::from(rule.proxy.clone()),
            Cell::from(rule.hits.to_string()),
        ])
        .style(if rule.disabled {
            Style::default().fg(MUTED)
        } else {
            Style::default()
        })
    });
    render_table(
        frame,
        area,
        " Rules · Space/x temporarily toggle ",
        ["#", "State", "Type", "Payload", "Policy", "Hits"],
        rows,
        [
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Length(15),
            Constraint::Percentage(48),
            Constraint::Percentage(22),
            Constraint::Length(7),
        ],
        app.selected[app.tab_index()],
    );
}

fn draw_providers(frame: &mut Frame, app: &App, area: Rect) {
    let kind = if app.proxy_provider_view {
        "Proxy"
    } else {
        "Rule"
    };
    let rows = app.filtered_providers().into_iter().map(|provider| {
        Row::new([
            Cell::from(provider.name.clone()),
            Cell::from(provider.kind.clone()),
            Cell::from(provider.vehicle.clone()),
            Cell::from(provider.count.to_string()),
            Cell::from(provider.updated_at.clone()),
        ])
    });
    render_table(
        frame,
        area,
        &format!(" {kind} providers · p switch type · u update · h healthcheck "),
        ["Name", "Type", "Vehicle", "Items", "Updated"],
        rows,
        [
            Constraint::Percentage(30),
            Constraint::Length(13),
            Constraint::Length(12),
            Constraint::Length(8),
            Constraint::Percentage(35),
        ],
        app.selected[app.tab_index()],
    );
}

fn draw_profiles(frame: &mut Frame, app: &App, area: Rect) {
    if app.local.is_none() {
        frame.render_widget(
            Paragraph::new("Profiles are local Clash Verge data. Pass --data-dir when using a remote controller.")
                .block(Block::default().borders(Borders::ALL).title(" Profiles "))
                .wrap(Wrap { trim: true }),
            area,
        );
        return;
    }
    let rows = app.filtered_profiles().into_iter().map(|item| {
        let current = app.profiles.current.as_deref() == Some(&item.uid);
        Row::new([
            Cell::from(if current { "●" } else { " " }),
            Cell::from(item.name.clone()),
            Cell::from(item.kind.clone()),
            Cell::from(item.uid.clone()),
            Cell::from(item.updated.map(format_epoch).unwrap_or_else(|| "—".into())),
            Cell::from(if item.url.is_some() {
                "remote"
            } else {
                "local"
            }),
        ])
    });
    render_table(
        frame,
        area,
        " Profiles · a import · u update · Enter mark current · x delete ",
        ["", "Name", "Type", "UID", "Updated", "Source"],
        rows,
        [
            Constraint::Length(2),
            Constraint::Percentage(32),
            Constraint::Length(10),
            Constraint::Length(15),
            Constraint::Length(17),
            Constraint::Length(8),
        ],
        app.selected[app.tab_index()],
    );
}

fn draw_logs(frame: &mut Frame, app: &App, area: Rect) {
    let filtered = app.filtered_logs();
    let selected = app.selected[app.tab_index()].min(filtered.len().saturating_sub(1));
    let items = filtered
        .into_iter()
        .map(|line| ListItem::new(line.as_str()))
        .collect::<Vec<_>>();
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Clash Verge logs · / filter "),
        )
        .highlight_style(Style::default().bg(Color::DarkGray));
    let mut state = ListState::default().with_selected((!app.logs.is_empty()).then_some(selected));
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_settings(frame: &mut Frame, app: &App, area: Rect) {
    let local = app
        .local
        .as_ref()
        .map(|store| store.data_dir().display().to_string())
        .unwrap_or_else(|| "remote/core-only".into());
    let lines = vec![
        Line::styled(
            "Runtime toggles",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Line::from(format!(
            "  l   allow LAN       {}",
            on_off(app.snapshot.allow_lan)
        )),
        Line::from(format!(
            "  v   IPv6            {}",
            on_off(app.snapshot.ipv6)
        )),
        Line::from(""),
        Line::styled(
            "Maintenance (confirmation required)",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Line::from("  d   flush DNS cache"),
        Line::from("  f   flush fake-IP cache"),
        Line::from("  g   update GEO databases"),
        Line::from("  u   upgrade Mihomo core"),
        Line::from("  R   restart Mihomo core"),
        Line::from(""),
        Line::styled(
            "Integration",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Line::from(format!("  data directory      {local}")),
        Line::from("  local GUI settings not exposed by Mihomo remain owned by Clash Verge"),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Settings & maintenance "),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    let hints = match app.tab {
        Tab::Home => "m mode  t TUN  s system proxy",
        Tab::Proxies => "Enter open/select  d delay  Esc back",
        Tab::Connections => "x close  X close all",
        Tab::Rules => "Space/x toggle runtime rule",
        Tab::Providers => "p proxy/rule  u update  h health",
        Tab::Profiles => "a import  u update  Enter current  x delete",
        Tab::Logs => "j/k scroll",
        Tab::Settings => "d/f caches  g GEO  u upgrade  R restart",
    };
    let message = app.error.as_deref().unwrap_or(&app.status);
    let style = if app.error.is_some() {
        Style::default().fg(Color::Red)
    } else {
        Style::default().fg(MUTED)
    };
    let line = Line::from(vec![
        Span::styled(format!(" {message}"), style),
        Span::raw("  │  "),
        Span::styled(hints, Style::default().fg(ACCENT)),
        Span::raw("  │  / search  r refresh  ? help  q quit"),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_help(frame: &mut Frame, area: Rect) {
    draw_popup(
        frame,
        area,
        "Keyboard",
        vec![
            Line::from("1–8 / Tab      switch page"),
            Line::from("j/k / ↑/↓      move selection"),
            Line::from("g/G            first/last item"),
            Line::from("/              search current data"),
            Line::from("r              refresh all controller data"),
            Line::from("Esc            back/cancel"),
            Line::from("q / Ctrl-C     quit"),
            Line::from(""),
            Line::styled("Press any key to close", Style::default().fg(ACCENT)),
        ],
        54,
        13,
    );
}

fn draw_popup(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    lines: Vec<Line<'_>>,
    width: u16,
    height: u16,
) {
    let popup = centered(area, width, height);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Left)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" {title} ")),
            )
            .wrap(Wrap { trim: true }),
        popup,
    );
}

fn render_table<const N: usize, I>(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    headers: [&str; N],
    rows: I,
    widths: [Constraint; N],
    selected: usize,
) where
    I: IntoIterator<Item = Row<'static>>,
{
    let header = Row::new(headers)
        .style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .bottom_margin(1);
    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(title))
        .row_highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("› ");
    let mut state = TableState::default().with_selected(Some(selected));
    frame.render_stateful_widget(table, area, &mut state);
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width.saturating_sub(2)).max(1);
    let height = height.min(area.height.saturating_sub(2)).max(1);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

fn on_off(enabled: bool) -> &'static str {
    if enabled { "on" } else { "off" }
}

fn bool_style(enabled: bool) -> Style {
    Style::default().fg(if enabled { Color::Green } else { Color::Red })
}

fn value_or_dash(value: &str) -> &str {
    if value.is_empty() { "—" } else { value }
}

fn human_bytes(value: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut amount = value as f64;
    let mut unit = 0;
    while amount >= 1024.0 && unit < UNITS.len() - 1 {
        amount /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value} B")
    } else {
        format!("{amount:.1} {}", UNITS[unit])
    }
}

fn short_process(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_owned()
}

fn format_epoch(value: u64) -> String {
    chrono::DateTime::from_timestamp(value as i64, 0)
        .map(|time| {
            time.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "—".into())
}

trait AppUiExt {
    fn tab_index(&self) -> usize;
}

impl AppUiExt for App {
    fn tab_index(&self) -> usize {
        Tab::ALL
            .iter()
            .position(|tab| *tab == self.tab)
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_are_human_readable() {
        assert_eq!(human_bytes(500), "500 B");
        assert_eq!(human_bytes(1536), "1.5 KiB");
    }
}
