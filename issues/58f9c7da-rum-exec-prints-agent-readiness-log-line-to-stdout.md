# rum exec prints agent readiness log line to stdout

**ID:** 58f9c7da | **Status:** Open | **Created:** 2026-02-24T20:41:07+01:00

Every `rum exec` invocation prints an agent readiness log line before the actual command output:
```
2026-02-24T19:22:28.141561Z  INFO rum::agent: agent ready version=0.1.0 hostname=vibedb
```
This breaks agents/scripts that parse stdout. The log line should go to stderr or be suppressed entirely for `exec`.
