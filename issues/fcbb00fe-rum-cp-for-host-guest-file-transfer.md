# rum cp for host-guest file transfer

**ID:** fcbb00fe | **Status:** Done | **Created:** 2026-02-24T20:41:09+01:00

Add `rum cp` for one-off file transfers without needing persistent `[[mounts]]`:

```
rum cp localfile.txt :/remote/path/
rum cp :/remote/file.txt ./local/
```

Could use the vsock agent to transfer file contents, or tunnel via SSH/SCP.
