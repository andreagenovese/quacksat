# ADR 0005: Mappa e localizzazione — consumare il maploc di robotd

- Stato: accettata, poi spostata
- Data: 2026-09-04; spostata il 2026-09-22 (ADR 0006)

La decisione che questo verbale contiene — prendere il `maploc` di
robotd come mappa e come posa invece di costruirci uno SLAM nostro, e
mettere il client della mappa, il registro dei posti e la guardia del
dirupo in un crate senza dipendenze dalla voce — è della navigazione, e
la navigazione ha lasciato questo repo il 2026-09-22. Il verbale se
n'è andato con lei, insieme a quel che ne è nato: l'esploratore, la
legge del passaggio, i libri del terreno, l'homecoming, il gemello di
carta e tre settimane di misure sul gemello MuJoCo.

Vive nel repo della navigazione:

    https://github.com/andreagenovese/quacknav   docs/adr/

Il numero resta qui perché l'ADR 0006 e `docs/agent-protocol.it.md` lo
citano, e perché una numerazione con un buco invita a pensare che una
decisione non sia mai stata scritta.
