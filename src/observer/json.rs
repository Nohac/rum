//! JSON-lines observer — structured output for machine consumption.

use std::future::Future;
use std::pin::Pin;

use super::{EffectData, Observer, Transition};

pub struct JsonObserver;

impl Observer for JsonObserver {
    fn on_transition(&mut self, _t: &Transition) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async { todo!("JsonObserver::on_transition") })
    }

    fn on_effect_stream(&mut self, _name: &str, _rx: roam::Rx<EffectData>) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async { todo!("JsonObserver::on_effect_stream") })
    }

    fn clone_for_stream(&self) -> Box<dyn Observer> {
        todo!("JsonObserver::clone_for_stream")
    }
}
