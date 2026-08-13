mod api;
mod app;
mod config;
mod local;
mod model;
mod ui;

use std::io::{self, IsTerminal};

use anyhow::{Context, Result, bail};
use clap::Parser;
use crossterm::{
    event::{self, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::{
    api::MihomoClient,
    app::App,
    config::{Args, Settings},
    local::LocalStore,
};

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let doctor = args.doctor;
    let settings = Settings::discover(args)?;
    if doctor {
        println!("{}", settings.doctor());
        return Ok(());
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("clash-verge-tui requires an interactive terminal (try --doctor for diagnostics)");
    }

    install_panic_hook();
    let client = MihomoClient::new(settings.controller, settings.secret)?;
    let local = settings.data_dir.map(LocalStore::new).transpose()?;
    let mut app = App::new(client, local);
    app.refresh().await;

    let mut terminal = init_terminal()?;
    let run_result = run(&mut terminal, &mut app, settings.refresh_interval).await;
    let restore_result = restore_terminal(&mut terminal);
    run_result.and(restore_result)
}

fn install_panic_hook() {
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original_hook(panic_info);
    }));
}

async fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    refresh_interval: std::time::Duration,
) -> Result<()> {
    loop {
        if app.take_full_redraw() {
            terminal
                .clear()
                .context("clear terminal before full redraw")?;
        }
        terminal.draw(|frame| ui::draw(frame, app))?;
        if app.quit {
            return Ok(());
        }

        let until_refresh = refresh_interval.saturating_sub(app.last_refresh.elapsed());
        let wait = until_refresh.min(std::time::Duration::from_millis(200));
        if event::poll(wait).context("poll terminal event")? {
            match event::read().context("read terminal event")? {
                Event::Key(key) if key.kind == event::KeyEventKind::Press => {
                    app.handle_key(key).await
                }
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
        if app.last_refresh.elapsed() >= refresh_interval
            && app.input_mode.is_none()
            && app.confirm.is_none()
        {
            app.refresh().await;
        }
    }
}

fn init_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode().context("enable terminal raw mode")?;
    let mut stdout = io::stdout();
    if let Err(error) = execute!(stdout, EnterAlternateScreen) {
        let _ = disable_raw_mode();
        return Err(error).context("enter alternate screen");
    }
    match Terminal::new(CrosstermBackend::new(stdout)) {
        Ok(terminal) => Ok(terminal),
        Err(error) => {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
            Err(error).context("initialize terminal")
        }
    }
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode().context("disable terminal raw mode")?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen).context("leave alternate screen")?;
    terminal.show_cursor().context("show terminal cursor")?;
    Ok(())
}
