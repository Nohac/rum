pub(super) struct WizardConfig {
    pub(super) image_url: String,
    pub(super) image_comment: Option<String>,
    pub(super) cpus: u32,
    pub(super) memory_mb: u64,
    pub(super) disk: String,
    pub(super) hostname: String,
    pub(super) nat: bool,
    pub(super) interfaces: Vec<WizardInterface>,
    pub(super) mounts: Vec<WizardMount>,
    pub(super) drives: Vec<WizardDrive>,
    pub(super) filesystems: Vec<WizardFs>,
}

pub(super) struct WizardInterface {
    pub(super) network: String,
    pub(super) ip: String,
}

pub(super) struct WizardMount {
    pub(super) source: String,
    pub(super) target: String,
    pub(super) readonly: bool,
    pub(super) tag: String,
}

pub(super) struct WizardDrive {
    pub(super) name: String,
    pub(super) size: String,
}

pub(super) struct WizardFs {
    pub(super) fs_type: String,
    pub(super) drives: Vec<String>,
    pub(super) mount_target: String,
    pub(super) pool: String,
}

pub(super) enum WizardStep {
    OsImage,
    Resources,
    Hostname,
    Network,
    Mounts,
    Storage,
    Done,
}

impl WizardStep {
    pub(super) fn next(&self) -> Self {
        match self {
            Self::OsImage => Self::Resources,
            Self::Resources => Self::Hostname,
            Self::Hostname => Self::Network,
            Self::Network => Self::Mounts,
            Self::Mounts => Self::Storage,
            Self::Storage | Self::Done => Self::Done,
        }
    }

    pub(super) fn prev(&self) -> Self {
        match self {
            Self::OsImage => Self::OsImage,
            Self::Resources => Self::OsImage,
            Self::Hostname => Self::Resources,
            Self::Network => Self::Hostname,
            Self::Mounts => Self::Network,
            Self::Storage => Self::Mounts,
            Self::Done => Self::Storage,
        }
    }
}
