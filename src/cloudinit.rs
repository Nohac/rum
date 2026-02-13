use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;

use hadris_iso::read::PathSeparator;
use hadris_iso::write::options::{CreationFeatures, FormatOptions};
use hadris_iso::write::{File as IsoFile, InputFiles, IsoImageWriter};

use crate::error::RumError;

/// Generate a cloud-init NoCloud seed ISO (ISO 9660 with volume label "CIDATA").
pub async fn generate_seed_iso(
    seed_path: &Path,
    hostname: &str,
    provision_script: &str,
    packages: &[String],
) -> Result<(), RumError> {
    if let Some(parent) = seed_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| RumError::Io {
                context: format!("creating directory {}", parent.display()),
                source: e,
            })?;
    }

    let meta_data = format!("instance-id: {hostname}\nlocal-hostname: {hostname}\n");
    let user_data = build_user_data(provision_script, packages);

    let iso = build_iso(&meta_data, &user_data)?;

    tokio::fs::write(seed_path, &iso)
        .await
        .map_err(|e| RumError::Io {
            context: format!("writing seed ISO to {}", seed_path.display()),
            source: e,
        })?;

    tracing::info!(path = %seed_path.display(), "generated cloud-init seed ISO");
    Ok(())
}

fn build_user_data(provision_script: &str, packages: &[String]) -> String {
    let mut ud = String::from("#cloud-config\n");
    ud.push_str("users:\n");
    ud.push_str("  - name: rum\n");
    ud.push_str("    plain_text_passwd: rum\n");
    ud.push_str("    lock_passwd: false\n");
    ud.push_str("    shell: /bin/bash\n");
    ud.push_str("    sudo: ALL=(ALL) NOPASSWD:ALL\n");

    if !packages.is_empty() {
        ud.push_str("packages:\n");
        for pkg in packages {
            ud.push_str(&format!(
                "  - \"{}\"\n",
                pkg.replace('\\', "\\\\").replace('"', "\\\"")
            ));
        }
    }

    if !provision_script.is_empty() {
        ud.push_str("write_files:\n");
        ud.push_str("  - path: /var/lib/cloud/scripts/rum-provision.sh\n");
        ud.push_str("    permissions: \"0755\"\n");
        ud.push_str("    content: |\n");
        for line in provision_script.lines() {
            if line.is_empty() {
                ud.push('\n');
            } else {
                ud.push_str(&format!("      {line}\n"));
            }
        }
        ud.push_str("runcmd:\n");
        ud.push_str("  - [\"/bin/bash\", \"/var/lib/cloud/scripts/rum-provision.sh\"]\n");
    }

    ud
}

fn build_iso(meta_data: &str, user_data: &str) -> Result<Vec<u8>, RumError> {
    let files = InputFiles {
        path_separator: PathSeparator::ForwardSlash,
        files: vec![
            IsoFile::File {
                name: Arc::new("meta-data".to_string()),
                contents: meta_data.as_bytes().to_vec(),
            },
            IsoFile::File {
                name: Arc::new("user-data".to_string()),
                contents: user_data.as_bytes().to_vec(),
            },
        ],
    };

    let format_options = FormatOptions {
        volume_name: "CIDATA".to_string(),
        sector_size: 2048,
        path_seperator: PathSeparator::ForwardSlash,
        features: CreationFeatures::default(),
    };

    // hadris-iso reads back during formatting, so the buffer must be pre-allocated
    let mut buf = Cursor::new(vec![0u8; 256 * 2048]);
    IsoImageWriter::format_new(&mut buf, files, format_options).map_err(|e| RumError::Io {
        context: "generating seed ISO".into(),
        source: std::io::Error::other(e.to_string()),
    })?;

    Ok(buf.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_data_contains_user() {
        let ud = build_user_data("", &[]);
        assert!(ud.contains("name: rum"));
        assert!(ud.contains("plain_text_passwd: rum"));
        assert!(ud.contains("lock_passwd: false"));
    }

    #[test]
    fn user_data_packages_are_quoted() {
        let ud = build_user_data("", &["curl".into(), "git".into()]);
        assert!(ud.contains("packages:"));
        assert!(ud.contains("  - \"curl\""));
        assert!(ud.contains("  - \"git\""));
    }

    #[test]
    fn user_data_yaml_special_chars_in_packages() {
        let ud = build_user_data("", &["foo: bar".into(), "baz\"qux".into()]);
        assert!(ud.contains("  - \"foo: bar\""));
        assert!(ud.contains("  - \"baz\\\"qux\""));
    }

    #[test]
    fn user_data_provision_uses_write_files() {
        let ud = build_user_data("echo hello\necho world", &[]);
        assert!(ud.contains("write_files:"));
        assert!(ud.contains("  - path: /var/lib/cloud/scripts/rum-provision.sh"));
        assert!(ud.contains("    content: |\n"));
        assert!(ud.contains("      echo hello\n"));
        assert!(ud.contains("      echo world\n"));
        assert!(ud.contains("runcmd:"));
        assert!(ud.contains(
            "  - [\"/bin/bash\", \"/var/lib/cloud/scripts/rum-provision.sh\"]"
        ));
    }

    #[test]
    fn user_data_multiline_script_preserved() {
        let script = "if true; then\n  echo yes\nfi";
        let ud = build_user_data(script, &[]);
        assert!(ud.contains("      if true; then\n"));
        assert!(ud.contains("        echo yes\n"));
        assert!(ud.contains("      fi\n"));
    }

    #[test]
    fn iso_is_generated() {
        let iso = build_iso("instance-id: test\n", "#cloud-config\n").unwrap();
        // ISO 9660 images start with 32 KiB of system area, then "CD001" magic
        assert!(iso.len() > 32 * 1024);
        assert_eq!(&iso[0x8001..0x8006], b"CD001");
    }
}
