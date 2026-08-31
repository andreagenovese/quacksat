# ADR 0001: Repository separato, non un fork di pollen-robotics/microduck

- Stato: accettato
- Data: 2026-08-31

## Contesto

quacksat gira sul Microduck e dialoga con `robotd`, il demone di sistema di
`pollen-robotics/microduck`. Sono stati considerati due modi di strutturare
il codice: fare un fork del monorepo upstream e aggiungere quacksat al suo
interno, oppure tenere quacksat in un repository proprio che dipende dai
crate upstream.

## Decisione

quacksat vive in un repository proprio. Dipende dai crate upstream
(`duck-ipc-proto`) e, quando servono modifiche upstream, le patch vanno a
`pollen-robotics/microduck` come pull request — mai come divergenza di
lungo periodo di un fork.

## Conseguenze

- quacksat ha una propria cadenza di release, un proprio header di licenza
  e un proprio issue tracker, e resta chiaramente indipendente (nessuna
  affiliazione implicita con Pollen Robotics).
- I refactoring upstream (per es. la prevista migrazione audio verso
  `mediad`, milestone M5) vengono assorbiti come aggiornamenti di
  dipendenza invece che come dolorosi rebase del fork.
- La superficie IPC di `robotd` è l'unico contratto su cui quacksat fa
  affidamento; tutto ciò che non è raggiungibile attraverso
  `/run/robotd.sock` richiede prima una PR upstream.
- Gli aggiornamenti sono pacchettizzati come unit systemd separata fuori
  da `releases/` di upstream, reinstallabile tramite `updaterd`.
