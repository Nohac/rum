use inquire::Confirm;

use crate::error::RumError;
use super::errors::map_inquire_err;

pub(super) fn detect_backend() -> Result<(), RumError> {
    let kvm_available = std::path::Path::new("/dev/kvm").exists();

    let libvirt_available = {
        virt::error::clear_error_callback();
        virt::connect::Connect::open(Some("qemu:///system"))
            .map(|mut c| {
                let _ = c.close();
            })
            .is_ok()
    };

    if kvm_available && libvirt_available {
        println!("  Detected: KVM + libvirt (qemu:///system)");
    } else {
        if !kvm_available {
            println!("  Warning: KVM not available (/dev/kvm not found)");
        }
        if !libvirt_available {
            println!("  Warning: Cannot connect to libvirt (qemu:///system)");
        }
        let proceed = Confirm::new("Continue anyway?")
            .with_default(false)
            .prompt()
            .map_err(map_inquire_err)?;
        if !proceed {
            return Err(RumError::InitCancelled);
        }
    }

    println!();
    Ok(())
}
