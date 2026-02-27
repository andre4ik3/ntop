use crate::ps;
use anyhow::Context;
use chrono::TimeDelta;
use crossterm::event::{Event as TerminalEvent, KeyCode, KeyEvent, KeyModifiers};
use futures::{FutureExt, StreamExt};
use std::fmt::format;
use std::time::Duration;
use tokio::{sync::mpsc, time};

use ratatui::layout::Constraint;
use ratatui::prelude::Color;
use ratatui::text::Line;
use ratatui::widgets::{Cell, Row, Table, TableState};
use ratatui::{
    DefaultTerminal, Frame,
    layout::Alignment,
    style::{Style, Stylize},
    widgets::{Block, BorderType, StatefulWidget, Widget},
};

#[derive(Debug)]
pub enum AppEvent {
    Refresh(anyhow::Result<ps::Output>),
    Quit,
}

#[derive(Debug)]
pub enum Event {
    Terminal(TerminalEvent),
    App(AppEvent),
}

#[derive(Debug)]
pub struct App {
    running: bool,

    // event plumbing
    sender: mpsc::UnboundedSender<Event>,
    receiver: mpsc::UnboundedReceiver<Event>,

    // getting builds
    pub refresh_interval: Duration,
    pub active_builds: Vec<ps::Build>,

    // stuff
    pub table_state: TableState,
}

impl App {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        Self {
            running: true,
            sender,
            receiver,
            refresh_interval: Duration::from_secs(5),
            active_builds: Vec::new(),
            table_state: TableState::default(),
        }
    }

    /// Run the application's main loop.
    pub async fn run(mut self, mut terminal: DefaultTerminal) -> anyhow::Result<()> {
        // terminal event task thing
        let sender = self.sender.clone();
        tokio::spawn(async move {
            let mut reader = crossterm::event::EventStream::new();
            loop {
                tokio::select! {
                    _ = sender.closed() => break,
                    Some(Ok(evt)) = reader.next().fuse() => {
                        _ = sender.send(Event::Terminal(evt));
                    }
                }
            }
        });

        // send initial data
        _ = self
            .sender
            .send(Event::App(AppEvent::Refresh(ps::get().await)));

        while self.running {
            terminal.draw(|frame| self.render(frame))?;
            match self
                .receiver
                .recv()
                .await
                .context("while receiving event")?
            {
                Event::Terminal(event) => match event {
                    crossterm::event::Event::Key(key_event)
                        if key_event.kind == crossterm::event::KeyEventKind::Press =>
                    {
                        self.handle_key_events(key_event)?
                    }
                    _ => {}
                },
                Event::App(app_event) => match app_event {
                    AppEvent::Refresh(output) => self.refresh(output),
                    AppEvent::Quit => break,
                },
            }
        }
        Ok(())
    }

    /// Handles terminal key events.
    fn handle_key_events(&mut self, key_event: KeyEvent) -> anyhow::Result<()> {
        match key_event.code {
            // refresh interval
            KeyCode::Char('-') => {
                let new = self
                    .refresh_interval
                    .saturating_sub(Duration::from_millis(100));

                if new.as_millis() >= 100 {
                    self.refresh_interval = new;
                }
            }
            KeyCode::Char('=' | '+') => {
                self.refresh_interval = self
                    .refresh_interval
                    .saturating_add(Duration::from_millis(100));
            }

            // active builds table
            KeyCode::Up => self.table_state.select_previous(),
            KeyCode::Down => self.table_state.select_next(),
            KeyCode::Esc => self.table_state.select(None),

            // quitting
            KeyCode::Char('q') => _ = self.sender.send(Event::App(AppEvent::Quit)),
            KeyCode::Char('c' | 'C') if key_event.modifiers == KeyModifiers::CONTROL => {
                _ = self.sender.send(Event::App(AppEvent::Quit));
            }
            _ => {}
        }
        Ok(())
    }

    /// Processes a received `nix ps` output and schedules the next one to run.
    fn refresh(&mut self, output: anyhow::Result<ps::Output>) {
        if let Ok(builds) = output {
            // TODO: handle errors
            self.active_builds = builds;
        }

        // schedule next refresh
        let duration = self.refresh_interval.clone();
        let sender = self.sender.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = sender.closed() => {},
                _ = time::sleep(duration) => {
                    // SELECT AGAIN !! to handle exiting mid-thing
                    tokio::select! {
                        _ = sender.closed() => {},
                        output = ps::get() => {
                            _ = sender.send(Event::App(AppEvent::Refresh(output)));
                        },
                    }
                }
            }
        });
    }

    fn render(&mut self, frame: &mut Frame) {
        let block = Block::bordered()
            .title_top(
                Line::from(vec![
                    "-".red(),
                    format!(" {}ms ", self.refresh_interval.as_millis()).white(),
                    "+".red(),
                ])
                .alignment(Alignment::Right),
            )
            .title_bottom(Line::from(vec!["↑".red(), " select ".white(), "↓".red()]))
            .border_type(BorderType::Rounded)
            .border_style(Style::new().black());

        let widths = [
            Constraint::Length(10),     // PID
            Constraint::Percentage(70), // pname
            Constraint::Percentage(30), // version
            Constraint::Length(10),     // time?
        ];

        let header = Row::new(vec![
            Cell::from("PID"),
            Cell::from("Package"),
            Cell::from("Version"),
            Cell::from("Time"),
        ])
        .dim()
        .underlined();

        let table = Table::new(&self.active_builds, widths)
            .block(block)
            .header(header)
            .row_highlight_style(Style::new().bg(Color::Rgb(19, 57, 117)));

        frame.render_stateful_widget(table, frame.area(), &mut self.table_state);
    }
}

impl<'a> Into<Row<'a>> for &ps::Build {
    fn into(self) -> Row<'a> {
        // drop hash prefix and .drv suffix
        let name = &self.derivation[33..self.derivation.len() - 4];
        let mut cells = vec![];
        cells.push(Cell::from(format!("{}", self.nix_pid)));

        if let Some((pname, version)) = name.rsplit_once('-') {
            cells.push(Cell::from(pname.to_string()).light_green());
            cells.push(Cell::from(version.to_string()).cyan());
        } else {
            cells.push(Cell::from(name.to_string()).light_green());
            cells.push(Cell::from(""));
        }
        cells.push(Cell::from(show_duration(self.elapsed())));
        Row::new(cells)
    }
}

fn show_duration(duration: TimeDelta) -> String {
    let mut duration = duration;
    let mut components = vec![];

    if duration.num_days() > 0 {
        components.push(format!("{}d", duration.num_days()));
        duration = duration - TimeDelta::days(duration.num_days());
    }

    if duration.num_hours() > 0 {
        components.push(format!("{}h", duration.num_hours()));
        duration = duration - TimeDelta::hours(duration.num_hours());
    }

    if duration.num_minutes() > 0 && components.len() < 2 {
        components.push(format!("{}m", duration.num_minutes()));
        duration = duration - TimeDelta::minutes(duration.num_minutes());
    }

    if duration.num_seconds() > 0 && components.len() < 2 {
        components.push(format!("{}s", duration.num_seconds()));
    }

    components.join(" ")
}
