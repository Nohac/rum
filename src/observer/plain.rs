//! Plain text observer — no ANSI, suitable for piped output.

use std::future::Future;
use std::pin::Pin;

use super::{EffectData, Observer, Transition};

pub struct PlainObserver;

impl Observer for PlainObserver {
    fn on_transition(&mut self, _t: &Transition) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async { todo!("PlainObserver::on_transition") })
    }

    fn on_effect_stream(&mut self, _name: &str, _rx: roam::Rx<EffectData>) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async { todo!("PlainObserver::on_effect_stream") })
    }

    fn clone_for_stream(&self) -> Box<dyn Observer> {
        todo!("PlainObserver::clone_for_stream")
    }
}
