#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachineState {
    Missing,
    ImageCached,
    Prepared,
    PartialBoot,
    Stopped,
    Running,
    StaleConfig,
}
