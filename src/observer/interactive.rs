//! Interactive TTY observer — spinners, progress bars, colored output.

use std::future::Future;
use std::pin::Pin;

use super::{EffectData, Observer, Transition};

pub struct InteractiveObserver {
    // Will hold Arc<shared indicatif state>
}

impl InteractiveObserver {
    pub fn new() -> Self {
        Self {}
    }
}

impl Observer for InteractiveObserver {
    fn on_transition(&mut self, _t: &Transition) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async { todo!("InteractiveObserver::on_transition") })
    }

    fn on_effect_stream(&mut self, _name: &str, _rx: roam::Rx<EffectData>) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async { todo!("InteractiveObserver::on_effect_stream") })
    }

    fn clone_for_stream(&self) -> Box<dyn Observer> {
        todo!("InteractiveObserver::clone_for_stream")
    }
}
