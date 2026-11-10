//! 제품 관측기의 tracing 사건만 받아 준비·Peek 순서를 제어한다. 제품 소스는 바꾸지 않는다.
use super::*;
use tracing::field::{Field, Visit};
use tracing_subscriber::{Layer, layer::Context};

#[derive(Default)]
struct Fields {
    message: String,
    read_id: u64,
    enter_down: bool,
}
impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}");
        }
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        if field.name() == "read_id" {
            self.read_id = value;
        }
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        if field.name() == "enter_down" {
            self.enter_down = value;
        }
    }
}

pub(super) struct Hooks(pub(super) Arc<std::sync::Mutex<Option<Arc<Round>>>>);
impl<S: tracing::Subscriber> Layer<S> for Hooks {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let round = self.0.lock().unwrap().clone();
        let Some(round) = round.filter(|round| round.id == fields.read_id) else { return };
        match fields.message.as_str() {
            "install_console_observer_ready" => {
                signal(&round.observer_ready);
                // tracing callback은 실제 관측기 thread에서 WaitForMultipleObjects 전에 실행된다.
                // 이 조건만 관측기를 보류하며 자연 순서 조건은 관측기 실행을 막지 않는다.
                if round.order == "peek_after_pending" {
                    wait(&round.observer_release, round.deadline);
                }
            }
            "install_console_input_observed" if fields.enter_down => signal(&round.peeked_enter),
            _ => {}
        }
    }
}
