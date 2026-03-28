use std::path::Path;

use ssh_key::private::Ed25519Keypair;
use ssh_key::PrivateKey;

use crate::error::Error;

pub async fn ensure_ssh_keypair(key_path: &Path) -> Result<(), Error> {
    if key_path.exists() {
        return Ok(());
    }

    if let Some(parent) = key_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| Error::Io {
                context: format!("creating directory {}", parent.display()),
                source: e,
            })?;
    }

    let keypair = Ed25519Keypair::random(&mut rand_core::OsRng);
    let private = PrivateKey::from(keypair);

    let openssh_private = private
        .to_openssh(ssh_key::LineEnding::LF)
        .map_err(|e| Error::Io {
            context: format!("encoding SSH private key: {e}"),
            source: std::io::Error::other(e.to_string()),
        })?;
    tokio::fs::write(key_path, openssh_private.as_bytes())
        .await
        .map_err(|e| Error::Io {
            context: format!("writing SSH key to {}", key_path.display()),
            source: e,
        })?;

    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(key_path, std::fs::Permissions::from_mode(0o600))
            .await
            .map_err(|e| Error::Io {
                context: format!("setting permissions on {}", key_path.display()),
                source: e,
            })?;
    }

    let pub_key = private.public_key().to_openssh().map_err(|e| Error::Io {
        context: format!("encoding SSH public key: {e}"),
        source: std::io::Error::other(e.to_string()),
    })?;
    let pub_path = key_path.with_extension("pub");
    tokio::fs::write(&pub_path, pub_key.as_bytes())
        .await
        .map_err(|e| Error::Io {
            context: format!("writing SSH public key to {}", pub_path.display()),
            source: e,
        })?;

    tracing::info!(path = %key_path.display(), "generated SSH keypair");
    Ok(())
}

pub async fn collect_ssh_keys(key_path: &Path, extra_keys: &[String]) -> Result<Vec<String>, Error> {
    let pub_path = key_path.with_extension("pub");
    let auto_pub = tokio::fs::read_to_string(&pub_path)
        .await
        .map_err(|e| Error::Io {
            context: format!("reading SSH public key from {}", pub_path.display()),
            source: e,
        })?;
    let mut keys = vec![auto_pub.trim().to_string()];
    keys.extend(extra_keys.iter().cloned());
    Ok(keys)
}
