use super::windows_paste::{Item, Paste};
use std::time::{Duration, Instant};

use crossterm_winapi::{Console, Handle, InputRecord, KeyEventRecord};

use crate::event::{
    sys::windows::{parse::MouseButtonsPressed, poll::WinApiPoll},
    Event,
};

#[cfg(feature = "event-stream")]
use crate::event::sys::Waker;
use crate::event::{
    source::EventSource,
    sys::windows::parse::{handle_key_event, handle_mouse_event},
    timeout::PollTimeout,
    InternalEvent,
};

pub(crate) struct WindowsEventSource {
    console: Console,
    poll: WinApiPoll,
    surrogate_buffer: Option<u16>,
    mouse_buttons_pressed: MouseButtonsPressed,
    paste: Paste<KeyEventRecord>,
}

impl WindowsEventSource {
    pub(crate) fn new() -> std::io::Result<WindowsEventSource> {
        let console = Console::from(Handle::current_in_handle()?);
        Ok(WindowsEventSource {
            console,

            #[cfg(not(feature = "event-stream"))]
            poll: WinApiPoll::new(),
            #[cfg(feature = "event-stream")]
            poll: WinApiPoll::new()?,

            surrogate_buffer: None,
            mouse_buttons_pressed: MouseButtonsPressed::default(),
            paste: Paste::default(),
        })
    }
}

impl EventSource for WindowsEventSource {
    fn try_read(&mut self, timeout: Option<Duration>) -> std::io::Result<Option<InternalEvent>> {
        let poll_timeout = PollTimeout::new(timeout);

        loop {
            self.paste.expire(Instant::now());
            if let Some(item) = self.paste.ready.pop_front() {
                match item {
                    Item::Paste(text) => return Ok(Some(InternalEvent::Event(Event::Paste(text)))),
                    Item::Key(record) => {
                        if let Some(event) = handle_key_event(record, &mut self.surrogate_buffer) {
                            return Ok(Some(InternalEvent::Event(event)));
                        }
                    }
                }
                continue;
            }
            let wait = match (poll_timeout.leftover(), self.paste.wait(Instant::now())) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
            if let Some(event_ready) = self.poll.poll(wait)? {
                let number = self.console.number_of_console_input_events()?;
                if event_ready && number != 0 {
                    let event = match self.console.read_single_input_event()? {
                        InputRecord::KeyEvent(record) => {
                            self.paste.key(
                                record.clone(),
                                record.u_char,
                                record.key_down,
                                record.repeat_count,
                                record.control_key_state.has_state(0x1f),
                                Instant::now(),
                            )?;
                            None
                        }
                        InputRecord::MouseEvent(record) => {
                            let mouse_event = handle_mouse_event(record, &self.mouse_buttons_pressed);
                            self.mouse_buttons_pressed = MouseButtonsPressed {
                                left: record.button_state.left_button(),
                                right: record.button_state.right_button(),
                                middle: record.button_state.middle_button(),
                            };

                            mouse_event
                        }
                        InputRecord::WindowBufferSizeEvent(record) => {
                            // windows starts counting at 0, unix at 1, add one to replicate unix behaviour.
                            Some(Event::Resize((record.size.x as i32 + 1) as u16, (record.size.y as i32 + 1) as u16))
                        }
                        InputRecord::FocusEvent(record) => {
                            let event = if record.set_focus { Event::FocusGained } else { Event::FocusLost };
                            Some(event)
                        }
                        _ => None,
                    };

                    if let Some(event) = event {
                        return Ok(Some(InternalEvent::Event(event)));
                    }
                }
            }

            if poll_timeout.elapsed() {
                return Ok(None);
            }
        }
    }

    #[cfg(feature = "event-stream")]
    fn waker(&self) -> Waker {
        self.poll.waker()
    }
}
