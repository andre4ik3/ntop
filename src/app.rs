use crate::ps;
use anyhow::Context;
use crossterm::event::{Event as TerminalEvent, KeyCode, KeyEvent, KeyModifiers};
use futures::{FutureExt, StreamExt};
use std::time::Duration;
use tokio::{sync::mpsc, time};

use ratatui::{
    DefaultTerminal,
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Color, Stylize},
    widgets::{Block, BorderType, Paragraph, Widget},
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
    sender: mpsc::UnboundedSender<Event>,
    receiver: mpsc::UnboundedReceiver<Event>,

    pub refresh_interval: Duration,
    pub active_builds: Vec<ps::Build>,
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
        }
    }

    fn send(&self, event: AppEvent) {
        _ = self.sender.send(Event::App(event));
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
            terminal.draw(|frame| frame.render_widget(&self, frame.area()))?;
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

    /// Handles the key events and updates the state of [`App`].
    pub fn handle_key_events(&mut self, key_event: KeyEvent) -> anyhow::Result<()> {
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

            // quitting
            KeyCode::Esc | KeyCode::Char('q') => _ = self.send(AppEvent::Quit),
            KeyCode::Char('c' | 'C') if key_event.modifiers == KeyModifiers::CONTROL => {
                self.send(AppEvent::Quit)
            }
            _ => {}
        }
        Ok(())
    }

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
}

impl Widget for &App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::bordered()
            .title("ntop")
            .title_alignment(Alignment::Center)
            .border_type(BorderType::Rounded);

        let text = format!(
            "This is a tui template.\n\
                Press `Esc`, `Ctrl-C` or `q` to stop running.\n\
                Press left and right to increment and decrement the counter respectively.\n\
                Refresh interval: {}ms, Active builds: {}",
            self.refresh_interval.as_millis(),
            self.active_builds.len()
        );

        let paragraph = Paragraph::new(text)
            .block(block)
            .fg(Color::Cyan)
            .bg(Color::Black)
            .centered();

        paragraph.render(area, buf);
    }
}
