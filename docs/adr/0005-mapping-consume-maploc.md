# ADR 0005: Mapping and localization — consume robotd's maploc

- Status: accepted, then moved
- Date: 2026-09-04; moved 2026-09-22 (ADR 0006)

The decision this record holds — take robotd's `maploc` as the map and
the pose rather than build a SLAM of our own, and put the map client,
the places registry and the cliff guard in a crate with no voice
dependencies — is the navigation's, and the navigation left this repo
on 2026-09-22. The record left with it, together with what grew out of
it: the explorer, the passage law, the ground books, the homecoming,
the paper twin and three weeks of measurements on the MuJoCo twin.

It lives in the navigation's own repo:

    https://github.com/andreagenovese/quacknav   docs/adr/

The number stays here because ADR 0006 and `docs/agent-protocol.md`
cite it, and because a numbering with a hole in it invites the guess
that a decision was never written down.
