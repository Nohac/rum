use std::fmt;
use std::io::IsTerminal;
use std::path::Path;

use indicatif::ProgressBar;
use inquire::Select;

use crate::error::RumError;

// ── Built-in presets ─────────────────────────────────────

struct Preset {
    label: &'static str,
    url: &'static str,
}

const PRESETS: &[Preset] = &[
    Preset {
        label: "Ubuntu 24.04 LTS (Noble)",
        url: "https://cloud-images.ubuntu.com/noble/current/noble-server-cloudimg-amd64.img",
    },
    Preset {
        label: "Ubuntu 22.04 LTS (Jammy)",
        url: "https://cloud-images.ubuntu.com/jammy/current/jammy-server-cloudimg-amd64.img",
    },
    Preset {
        label: "Fedora Cloud 43",
        url: "https://download.fedoraproject.org/pub/fedora/linux/releases/43/Cloud/x86_64/images/Fedora-Cloud-Base-Generic-43-1.6.x86_64.qcow2",
    },
    Preset {
        label: "Debian 12 (Bookworm)",
        url: "https://cloud.debian.org/images/cloud/bookworm/latest/debian-12-generic-amd64.qcow2",
    },
    Preset {
        label: "Arch Linux",
        url: "https://geo.mirror.pkgbuild.com/images/latest/Arch-Linux-x86_64-cloudimg.qcow2",
    },
    Preset {
        label: "Alpine Linux 3.21",
        url: "https://dl-cdn.alpinelinux.org/alpine/v3.21/releases/cloud/nocloud_alpine-3.21.3-x86_64-bios-cloudinit-r0.qcow2",
    },
    Preset {
        label: "Rocky Linux 9",
        url: "https://dl.rockylinux.org/pub/rocky/9/images/x86_64/Rocky-9-GenericCloud-Base.latest.x86_64.qcow2",
    },
    Preset {
        label: "AlmaLinux 9",
        url: "https://repo.almalinux.org/almalinux/9/cloud/x86_64/images/AlmaLinux-9-GenericCloud-latest.x86_64.qcow2",
    },
    Preset {
        label: "openSUSE Leap 15.6",
        url: "https://download.opensuse.org/distribution/leap/15.6/appliances/openSUSE-Leap-15.6-Minimal-VM.x86_64-Cloud.qcow2",
    },
    Preset {
        label: "CentOS Stream 9",
        url: "https://cloud.centos.org/centos/9-stream/x86_64/images/CentOS-Stream-GenericCloud-9-latest.x86_64.qcow2",
    },
];

/// Returns `(label, url)` pairs for use by `init.rs` and other callers.
pub fn preset_labels_and_urls() -> Vec<(&'static str, &'static str)> {
    PRESETS.iter().map(|p| (p.label, p.url)).collect()
}

// ── Cloud image type ─────────────────────────────────────

pub struct CloudImage {
    pub label: String,
    pub url: String,
}

impl fmt::Display for CloudImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

// ── Fedora API ───────────────────────────────────────────

#[derive(Debug, Default, facet::Facet)]
#[facet(default)]
struct FedoraRelease {
    #[facet(default)]
    version: String,
    #[facet(default)]
    arch: String,
    #[facet(default)]
    variant: String,
    #[facet(default)]
    subvariant: String,
    #[facet(default)]
    link: String,
}

async fn fetch_fedora_images() -> Vec<CloudImage> {
    let Ok(client) = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
    else {
        return Vec::new();
    };

    let resp = client
        .get("https://www.fedoraproject.org/releases.json")
        .send()
        .await;

    let body = match resp {
        Ok(r) => match r.text().await {
            Ok(t) => t,
            Err(_) => return Vec::new(),
        },
        Err(_) => return Vec::new(),
    };

    let releases: Vec<FedoraRelease> = match facet_json::from_str(&body) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };

    releases
        .into_iter()
        .filter(|r| {
            r.variant == "Cloud"
                && r.subvariant == "Cloud_Base"
                && r.arch == "x86_64"
                && r.link.contains("Generic")
                && r.link.ends_with(".qcow2")
        })
        .map(|r| CloudImage {
            label: format!("Fedora Cloud {} (live)", r.version),
            url: r.link,
        })
        .collect()
}

// ── Registry assembly ────────────────────────────────────

async fn all_images() -> Vec<CloudImage> {
    let use_spinner = std::io::stderr().is_terminal();

    let spinner = if use_spinner {
        let s = ProgressBar::new_spinner();
        s.set_message("Fetching cloud image registry...");
        s.enable_steady_tick(std::time::Duration::from_millis(80));
        Some(s)
    } else {
        None
    };

    let fedora_images = fetch_fedora_images().await;

    if let Some(s) = spinner {
        s.finish_and_clear();
    }

    let mut images: Vec<CloudImage> = PRESETS
        .iter()
        .filter(|p| !p.label.starts_with("Fedora"))
        .map(|p| CloudImage {
            label: p.label.to_string(),
            url: p.url.to_string(),
        })
        .collect();

    if fedora_images.is_empty() {
        // Fall back to the built-in Fedora entry
        for p in PRESETS.iter().filter(|p| p.label.starts_with("Fedora")) {
            images.push(CloudImage {
                label: p.label.to_string(),
                url: p.url.to_string(),
            });
        }
    } else {
        images.extend(fedora_images);
    }

    // Sort: Ubuntu first, then alphabetical
    images.sort_by(|a, b| {
        let a_ubuntu = a.label.starts_with("Ubuntu");
        let b_ubuntu = b.label.starts_with("Ubuntu");
        match (a_ubuntu, b_ubuntu) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.label.cmp(&b.label),
        }
    });

    images
}

// ── Filtering ────────────────────────────────────────────

pub fn filter_images<'a>(images: &'a [CloudImage], query: Option<&str>) -> Vec<&'a CloudImage> {
    match query {
        None | Some("") => images.iter().collect(),
        Some(q) => {
            let q_lower = q.to_lowercase();
            images
                .iter()
                .filter(|img| img.label.to_lowercase().contains(&q_lower))
                .collect()
        }
    }
}

// ── Config updater ───────────────────────────────────────

pub fn update_config_base(
    config_path: &Path,
    new_url: &str,
    label: &str,
) -> Result<(), RumError> {
    if !config_path.exists() {
        println!("Selected: {label}");
        println!("URL: {new_url}");
        println!();
        println!("No rum.toml found. Add this to your [image] section:");
        println!("  base = \"{new_url}\"");
        return Ok(());
    }

    let content = std::fs::read_to_string(config_path).map_err(|e| RumError::ConfigLoad {
        path: config_path.display().to_string(),
        source: e,
    })?;

    let mut lines: Vec<String> = content.lines().map(String::from).collect();
    let mut in_image_section = false;
    let mut base_line_idx = None;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_image_section = trimmed == "[image]";
        }
        if in_image_section
            && trimmed.starts_with("base")
            && let Some(eq_pos) = trimmed.find('=')
        {
            let after_key = trimmed[..eq_pos].trim();
            if after_key == "base" {
                base_line_idx = Some(i);
                break;
            }
        }
    }

    match base_line_idx {
        Some(idx) => {
            // Replace the base line
            lines[idx] = format!("base = \"{new_url}\"");

            // Update or insert comment above
            if idx > 0 && lines[idx - 1].trim().starts_with('#') {
                lines[idx - 1] = format!("# {label}");
            }

            let mut output = lines.join("\n");
            if content.ends_with('\n') {
                output.push('\n');
            }

            std::fs::write(config_path, &output).map_err(|e| RumError::ConfigWrite {
                path: config_path.display().to_string(),
                source: e,
            })?;

            println!("Updated rum.toml: {label}");
        }
        None => {
            println!("Selected: {label}");
            println!("URL: {new_url}");
            println!();
            println!(
                "Could not find `base = ...` in [image] section of {}",
                config_path.display()
            );
            println!("Add this to your [image] section:");
            println!("  base = \"{new_url}\"");
        }
    }

    Ok(())
}

// ── Main entry point ─────────────────────────────────────

pub async fn search(query: Option<&str>, config_path: &Path) -> Result<(), RumError> {
    let images = all_images().await;
    let filtered = filter_images(&images, query);

    if filtered.is_empty() {
        let msg = match query {
            Some(q) => format!("No images matching '{q}'"),
            None => "No images available".to_string(),
        };
        return Err(RumError::Validation { message: msg });
    }

    // Non-interactive: print tab-separated list and exit
    if !std::io::stdout().is_terminal() {
        for img in &filtered {
            println!("{}\t{}", img.label, img.url);
        }
        return Ok(());
    }

    let labels: Vec<String> = filtered.iter().map(|img| img.label.clone()).collect();

    let choice = Select::new("Select an image:", labels)
        .with_help_message("Use ↑↓ to navigate, type to filter")
        .prompt()
        .map_err(map_inquire_err)?;

    let selected = filtered.iter().find(|img| img.label == choice).unwrap();

    update_config_base(config_path, &selected.url, &selected.label)?;

    Ok(())
}

fn map_inquire_err(e: inquire::InquireError) -> RumError {
    match e {
        inquire::InquireError::OperationCanceled | inquire::InquireError::OperationInterrupted => {
            RumError::InitCancelled
        }
        other => RumError::Validation {
            message: format!("prompt error: {other}"),
        },
    }
}

// ── Tests ────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_presets_not_empty() {
        let presets = preset_labels_and_urls();
        assert!(!presets.is_empty());
        for (label, url) in &presets {
            assert!(!label.is_empty());
            assert!(url.starts_with("https://"));
        }
    }

    #[test]
    fn filter_images_case_insensitive() {
        let images = vec![
            CloudImage {
                label: "Ubuntu 24.04".into(),
                url: "https://example.com/ubuntu".into(),
            },
            CloudImage {
                label: "Fedora Cloud 43".into(),
                url: "https://example.com/fedora".into(),
            },
        ];
        let result = filter_images(&images, Some("ubuntu"));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].label, "Ubuntu 24.04");

        let result = filter_images(&images, Some("FEDORA"));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].label, "Fedora Cloud 43");
    }

    #[test]
    fn filter_images_no_query_returns_all() {
        let images = vec![
            CloudImage {
                label: "Ubuntu".into(),
                url: "https://a".into(),
            },
            CloudImage {
                label: "Fedora".into(),
                url: "https://b".into(),
            },
        ];
        assert_eq!(filter_images(&images, None).len(), 2);
        assert_eq!(filter_images(&images, Some("")).len(), 2);
    }

    #[test]
    fn filter_images_no_match_returns_empty() {
        let images = vec![CloudImage {
            label: "Ubuntu".into(),
            url: "https://a".into(),
        }];
        assert!(filter_images(&images, Some("nixos")).is_empty());
    }

    #[test]
    fn update_config_base_replaces_url() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rum.toml");
        std::fs::write(
            &path,
            "[image]\n# Ubuntu 22.04\nbase = \"https://old.example.com/img.qcow2\"\n\n[resources]\ncpus = 2\nmemory_mb = 2048\n",
        )
        .unwrap();

        update_config_base(&path, "https://new.example.com/new.qcow2", "Fedora 43").unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("base = \"https://new.example.com/new.qcow2\""));
        assert!(content.contains("# Fedora 43"));
        assert!(!content.contains("old.example.com"));
        // Preserves other sections
        assert!(content.contains("[resources]"));
        assert!(content.contains("cpus = 2"));
    }

    #[test]
    fn update_config_nonexistent_file_prints_url() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.toml");

        // Should not error — just prints instructions
        update_config_base(&path, "https://example.com/img.qcow2", "Test Image").unwrap();
    }
}
