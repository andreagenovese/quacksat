# Gli appunti della navigazione si sono spostati

`docs/todo-map.it.md`, `docs/study/baseline-twin.it.md` e gli studi
`maploc-*` — tre settimane di misure sul gemello MuJoCo, la mappa, la
guardia del dirupo, l'esploratore, la tromba delle scale, il boot — se
ne sono andati col codice il 2026-09-22 (ADR 0006), e con loro
`docs/study/upstream-asks.it.md` (cosa la navigazione chiederebbe allo
stack di Pollen) e i disegnatori di mappe `mapshot.py`, `mosaic.py` e
`rooms.py`. Vivono nel repo della navigazione:

    https://github.com/andreagenovese/quacknav   (docs/)

Qui resta il satellite: audio, parola di richiamo, VAD, i backend, i
tool del corpo, e `[nav] socket` — la lane che raggiunge `quack-navd`
quando ne gira uno.
