use crate::ps;
use anyhow::Context;
use chrono::{TimeDelta, Utc};
use crossterm::event::{Event as TerminalEvent, KeyCode, KeyEvent, KeyModifiers};
use futures::{FutureExt, StreamExt};
use std::time::Duration;
use tokio::{sync::mpsc, time};

use ratatui::{
    DefaultTerminal, Frame,
    layout::{Alignment, Direction, Layout, Rect},
    macros::{constraint, constraints, line, row, text, vertical},
    style::{Color, Style, Stylize},
    text::{Line, Text},
    widgets::{Block, BorderType, Cell, Padding, Paragraph, Row, Table, TableState},
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
    pub direction: Direction,
    pub table_state: TableState,
}

impl App {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        Self {
            running: true,
            sender,
            receiver,
            refresh_interval: Duration::from_secs(2),
            active_builds: Vec::new(),
            direction: Direction::Vertical,
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
            KeyCode::Up | KeyCode::Char('k') => self.table_state.select_previous(),
            KeyCode::Down | KeyCode::Char('j') => self.table_state.select_next(),
            KeyCode::Esc => self.table_state.select(None),

            // flip direction
            KeyCode::Char('/') => {
                self.direction = match self.direction {
                    Direction::Horizontal => Direction::Vertical,
                    Direction::Vertical => Direction::Horizontal,
                };
            }

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
            let previous_selection = self
                .table_state
                .selected()
                .and_then(|i| self.active_builds.get(i))
                .map(|b| b.nix_pid);

            self.active_builds = builds;
            let new_selection = previous_selection
                .and_then(|pid| self.active_builds.iter().position(|b| b.nix_pid == pid));

            self.table_state.select(new_selection);
        }

        // schedule next refresh
        let duration = self.refresh_interval;
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

    fn render_builds(&mut self, frame: &mut Frame, rect: Rect) {
        let block = Block::bordered()
            .title_top(Line::from("Active builds").cyan())
            .title_top(
                Line::from(vec![
                    "-".red(),
                    format!(" {}ms ", self.refresh_interval.as_millis()).white(),
                    "+".red(),
                ])
                .alignment(Alignment::Right),
            )
            .title_bottom(line!["↑".red(), " select ".white(), "↓".red()])
            .title_bottom(line!["/".red(), " change layout".white()].alignment(Alignment::Right))
            .border_type(BorderType::Rounded)
            .border_style(Style::new().black())
            .padding(Padding::horizontal(1));

        let header = Row::new(vec![
            Cell::from(Text::raw("PID").alignment(Alignment::Right)),
            Cell::from("Package"),
            Cell::from("Version"),
            Cell::from("Time"),
        ])
        .dim()
        .underlined();

        let table = Table::new(
            &self.active_builds,
            constraints![
                ==7,
                ==80%,
                ==20%,
                ==10
            ],
        )
        .block(block)
        .header(header)
        .row_highlight_style(Style::new().bg(Color::Rgb(19, 57, 117)));

        frame.render_stateful_widget(table, rect, &mut self.table_state);
    }

    fn render_build_details(&self, frame: &mut Frame, rect: Rect, build: &ps::Build) {
        let block = Block::bordered()
            .title_top(Line::from("Build").cyan())
            .border_type(BorderType::Rounded)
            .border_style(Style::new().black())
            .padding(Padding::uniform(1));

        let layout = vertical![==5, ==100%].split(block.inner(rect));

        let rows = vec![
            row![
                text!("Derivation").alignment(Alignment::Right).dim(),
                format!("/nix/store/{}", build.derivation).magenta(),
            ],
            row![
                text!("Started at").alignment(Alignment::Right).dim(),
                format!("{}", build.started()).yellow(),
            ],
            row![
                text!("Main PID").alignment(Alignment::Right).dim(),
                format!("{}", build.main_pid),
            ],
            row![
                text!("Nix PID").alignment(Alignment::Right).dim(),
                format!("{}", build.nix_pid),
            ],
        ];

        let properties = Table::new(rows, constraints![==10, ==100%]);
        let p = Paragraph::new(render_tree(build, build.main_pid));

        frame.render_widget(block, rect);
        frame.render_widget(properties, layout[0]);
        frame.render_widget(p, layout[1]);
    }

    fn render_details(&self, frame: &mut Frame, rect: Rect) {
        if let Some(selected) = self
            .table_state
            .selected()
            .and_then(|i| self.active_builds.get(i))
        {
            self.render_build_details(frame, rect, selected);
        } else {
            let text = Text::raw("Select a build to show its details").dim();
            let area = rect.centered(constraint!(==text.width() as u16), constraint!(==1));
            frame.render_widget(text, area);
        }
    }

    fn render(&mut self, frame: &mut Frame) {
        let layout = Layout::new(self.direction, constraints![==40%, ==60%]).split(frame.area());
        self.render_builds(frame, layout[0]);
        self.render_details(frame, layout[1]);
    }
}

impl<'a> From<&'a ps::Build> for Row<'a> {
    fn from(value: &'a ps::Build) -> Row<'a> {
        // drop hash prefix and .drv suffix
        let name = &value.derivation[33..value.derivation.len() - 4];

        let (pname, version) = if let Some((pname, version)) = name.rsplit_once('-') {
            (pname, version)
        } else {
            (name, "")
        };

        row![
            text!(format!("{}", value.nix_pid)).alignment(Alignment::Right),
            pname.light_green(),
            version.light_cyan(),
            show_duration(Utc::now() - value.started()),
        ]
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

fn render_tree(build: &ps::Build, pid: usize) -> String {
    let mut components = vec![];

    let Some(top) = build.processes.iter().find(|p| p.pid == pid) else {
        return "".to_string();
    };

    components.push(top.argv.join(" "));

    let children: Vec<&ps::BuildProcess> = build
        .processes
        .iter()
        .filter(|p| p.parent_pid == pid)
        .collect();
    for (i, child) in children.iter().enumerate() {
        let last = i == children.len() - 1;
        let subtree = render_tree(build, child.pid);
        let mut lines = subtree.lines();
        if let Some(line) = lines.next() {
            if last {
                components.push(format!("└─── {line}"));
            } else {
                components.push(format!("├─── {line}"));
            }
        }
        for line in lines {
            if last {
                components.push(format!("     {line}"));
            } else {
                components.push(format!("│    {line}"));
            }
        }
    }

    components.join("\n")
}
