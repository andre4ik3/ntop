use crate::ps;
use anyhow::Context;
use crossterm::event::{Event as TerminalEvent, KeyCode, KeyEvent, KeyModifiers};
use futures::{FutureExt, StreamExt};
use std::time::Duration;
use tokio::{sync::mpsc, time};

use ratatui::text::Line;
use ratatui::widgets::{List, ListItem, ListState};
use ratatui::{
    layout::Alignment, style::{Style, Stylize}
    ,
    widgets::{Block, BorderType, StatefulWidget, Widget},
    DefaultTerminal,
    Frame,
};
use ratatui::prelude::Color;

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
    pub list_state: ListState,
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
            list_state: ListState::default(),
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
            KeyCode::Up => self.list_state.select_previous(),
            KeyCode::Down => self.list_state.select_next(),
            KeyCode::Esc => self.list_state.select(None),

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
                .alignment(Alignment::Right)
            )
            .title_bottom(Line::from(vec![
                "↑".red(),
                " select ".white(),
                "↓".red(),
            ]))
            .border_type(BorderType::Rounded)
            .border_style(Style::new().black());

        let list = List::new(&self.active_builds)
            .block(block)
            .highlight_style(Style::new().bg(Color::Rgb(19, 57, 117)))
            .repeat_highlight_symbol(true);

        frame.render_stateful_widget(list, frame.area(), &mut self.list_state);
    }
}

impl<'a> Into<ListItem<'a>> for &ps::Build {
    fn into(self) -> ListItem<'a> {
        // drop hash prefix and .drv suffix
        let name = &self.derivation[33..self.derivation.len() - 4];
        if let Some((pname, version)) = name.rsplit_once('-') {
            ListItem::new(format!("{} ({})", pname, version))
        } else {
            ListItem::new(format!("{}", name))
        }
    }
}
