# Image shorthand resolution should work or suggest rum image search

**ID:** 71efd5d5 | **Status:** Open | **Created:** 2026-02-24T20:41:06+01:00

`base = "ubuntu-24.04"` is listed in the skill doc as valid shorthand but errors with "base image not found: ubuntu-24.04 / file not found". The registry has built-in presets but they aren't resolved automatically during `rum up`.

Fix: either resolve shorthands against the built-in preset list at image download time, or improve the error message to suggest `rum image search`.
