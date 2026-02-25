# Readiness probes and health checks after rum up

**ID:** 66808f0b | **Status:** Open | **Created:** 2026-02-24T20:41:08+01:00

After `rum up -d`, the VM is "running" but services inside may not be ready. Add a way to define readiness checks:

```toml
[provision.healthcheck]
command = "systemctl is-active myservice"
interval_s = 2
timeout_s = 60
```

Or a CLI primitive: `rum wait --exec "systemctl is-active myservice" --timeout 60`
