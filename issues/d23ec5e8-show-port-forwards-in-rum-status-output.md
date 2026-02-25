# Show port forwards in rum status output

**ID:** d23ec5e8 | **Status:** Open | **Created:** 2026-02-24T20:41:09+01:00

`rum status` doesn't show active port forwards. Add a line like:
```
Ports: 127.0.0.1:8080 -> guest:8080
```
Helps agents verify connectivity paths without re-reading the config.
