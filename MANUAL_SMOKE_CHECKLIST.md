# OpenAIRAC 1.0.0 — Manual Smoke Checklist (X-Plane 12)

The automated release gate covers everything that can be verified
without a running simulator GUI. The items below require X-Plane 12's
user interface and were NOT faked by automation. Complete them on a
machine with X-Plane 12 and the OpenAIRAC layer installed.

Preconditions: OpenAIRAC 1.0.0 installed (see INSTALLATION.md);
X-Plane 12 present; the OpenAIRAC layer installed into Custom Data
(transactional install completed, `resolve_sim_world` reports
Consistent).

## Flight planning / FMS checks

1. **Fixes**: open the map or an FMC, search for an enroute fix from
   the active cycle (e.g. SAHEY or DEDHD at KSFO). Expect the fix to
   exist with correct coordinates.
2. **VOR/DME/NDB**: tune a VOR frequency near KSFO (e.g. 113.70 SGD
   or per the active cycle) and confirm reception; verify a DME readout.
3. **ILS**: select the ILS 28L at KSFO; confirm the localizer comes
   alive (ident audio/display) and the glideslope appears when the
   aircraft is configured with an SBAS/ILS receiver.
4. **LOC-only**: load a localizer-only approach (e.g. LDA/SDF or a
   LOC approach at a regional airport); confirm localizer guidance
   without glideslope.
5. **SID**: load a SID from KSFO (e.g. CIITY3 or a runway transition)
   in the FMC; verify the leg sequence matches the chart.
6. **STAR**: load a STAR into KSFO/KDEN/KJFK/KLAX/KORD; verify the leg
   sequence.
7. **RNAV approach**: load an R-series approach (e.g. RNP Z RWY 10R
   at KSFO); verify LPV/LNAV vertical guidance per the aircraft's
   SBAS configuration.
8. **ILS approach**: load I28L at KSFO; fly/observe the final segment.
9. **Hold**: load a hold at an enroute or missed-approach fix; verify
   the hold entry geometry in the FMC plan.
10. **Missed approach**: execute a missed approach segment (FM legs);
    verify the missed approach legs are present and flyable.
11. **Airway**: route via a Victor/Jet airway (e.g. V257 or a T-route);
    verify airway selection resolves between fixes.

## Installation / safety checks

12. **Existing navdata untouched**: confirm the simulator's previous
    navdata (e.g. Navigraph) is still present in backups and the
    rollback restores it byte-for-byte.
13. **Rollback**: run `bundle rollback` / installer rollback and
    confirm the previous layer returns; reinstall afterwards.

Record results (cycle number used, aircraft, pass/fail per item) in
the release notes when filing a release.
