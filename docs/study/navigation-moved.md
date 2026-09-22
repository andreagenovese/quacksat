# The navigation's notes moved

`docs/todo-map.md`, `docs/study/baseline-twin.md` and the `maploc-*`
studies — three weeks of measurements on the MuJoCo twin, the map, the
cliff guard, the explorer, the stairwell, the boot — left with the code
on 2026-09-22 (ADR 0006), and so did `docs/study/upstream-asks.md`
(what the navigation would ask of Pollen's stack) and the map plotters
`mapshot.py`, `mosaic.py` and `rooms.py`. They live in the
navigation's own repo:

    https://github.com/andreagenovese/quacknav   (docs/)

What stays here is the satellite: audio, wake word, VAD, the backends,
the body's own tools, and `[nav] socket` — the lane that reaches
`quack-navd` when one is running.
