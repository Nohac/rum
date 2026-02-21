# Pure Rust qcow2 overlay creation (remove qemu-img dependency)

**ID:** 9a0a1b87 | **Status:** Done | **Created:** 2026-02-20T21:19:26+01:00

`overlay.rs` shells out to `qemu-img create -f qcow2 -b <base> -F qcow2 <overlay>` to create a qcow2 overlay backed by a base image. This is the only remaining external tool dependency besides libvirt itself.

`qcow2.rs` already generates valid empty qcow2 v2 images in pure Rust. Extending it to support backing files is straightforward — set the backing file offset (byte 8) and length (byte 16) in the header, then write the path string immediately after the 72-byte header (still within cluster 0).

This would make rum fully self-contained for disk creation, matching the pure Rust ISO 9660 and qcow2 generators.
