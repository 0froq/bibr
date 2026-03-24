use std::time::Duration;

use anyhow::{anyhow, Result};
use crossterm::event::{self, Event, KeyEvent, KeyEventKind, MouseEvent};
use tokio::{sync::mpsc, task::JoinHandle};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiEvent {
    Input(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),
    Tick,
}

pub struct EventHandler {
    receiver: mpsc::UnboundedReceiver<Result<UiEvent>>,
    task: JoinHandle<()>,
    shutdown_sender: mpsc::Sender<()>,
}

impl EventHandler {
    pub fn new(tick_rate: Duration) -> Self {
        let (event_sender, receiver) = mpsc::unbounded_channel();
        let (shutdown_sender, mut shutdown_receiver) = mpsc::channel(1);

        let task = tokio::spawn(async move {
            loop {
                if shutdown_receiver.try_recv().is_ok() {
                    break;
                }

                let sender = event_sender.clone();
                let next = tokio::task::spawn_blocking(move || -> Result<Option<UiEvent>> {
                    if event::poll(tick_rate)? {
                        Ok(map_crossterm_event(event::read()?))
                    } else {
                        Ok(Some(UiEvent::Tick))
                    }
                })
                .await;

                match next {
                    Ok(Ok(Some(event))) => {
                        if sender.send(Ok(event)).is_err() {
                            break;
                        }
                    }
                    Ok(Ok(None)) => {}
                    Ok(Err(error)) => {
                        let _ = sender.send(Err(error));
                        break;
                    }
                    Err(error) => {
                        let _ = sender.send(Err(anyhow!(error)));
                        break;
                    }
                }
            }
        });

        Self {
            receiver,
            task,
            shutdown_sender,
        }
    }

    pub async fn next(&mut self) -> Result<UiEvent> {
        self.receiver.recv().await.unwrap_or(Ok(UiEvent::Tick))
    }

    pub async fn shutdown(mut self) {
        let _ = self.shutdown_sender.send(()).await;
        match tokio::time::timeout(Duration::from_millis(500), &mut self.task).await {
            Ok(_) => {}
            Err(_) => {
                // Timeout: abort the task and await best-effort
                self.task.abort();
                let _ = self.task.await;
            }
        }
    }
}

pub fn map_crossterm_event(event: Event) -> Option<UiEvent> {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => Some(UiEvent::Input(key)),
        Event::Mouse(mouse) => Some(UiEvent::Mouse(mouse)),
        Event::Resize(width, height) => Some(UiEvent::Resize(width, height)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn maps_pressed_key_events() {
        let event = Event::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));

        assert_eq!(map_crossterm_event(event), Some(UiEvent::Input(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE))));
    }

    #[test]
    fn maps_resize_events() {
        assert_eq!(map_crossterm_event(Event::Resize(120, 40)), Some(UiEvent::Resize(120, 40)));
    }

    #[test]
    fn maps_mouse_events() {
        use crossterm::event::{MouseButton, MouseEventKind};
        let mouse_event = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 10,
            row: 20,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        assert_eq!(map_crossterm_event(Event::Mouse(mouse_event)), Some(UiEvent::Mouse(mouse_event)));
    }
}
