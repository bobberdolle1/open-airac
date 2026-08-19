# OpenAIRAC — X-Plane 12 Navigation Data Gap Matrix

**Date:** 2026-08-16  
**Cycle Reference:** AIRAC 2608  
**Scope:** Forensic comparison of installed mature X-Plane 12 navdata (Navigraph AIRAC 2608) vs. OpenAIRAC 1.0 Capabilities & Open Data Sources (FAA CIFP, OurAirports).

---

## 1. Executive Summary

This gap matrix characterises every auxiliary dataset and extended row type discovered during the forensic inventory of Navigraph AIRAC 2608 for X-Plane 12. Each feature is evaluated against simulator correctness, source availability from open/legal authorities (FAA CIFP, OurAirports), and release urgency (1.0 vs. Post-1.0).

---

## 2. Detailed Dataset & Row Type Matrix

### 2.1. LPV Final Approach Segment (FAS) Data (`earth_nav.dat` Rows 14 & 16)
- **Source Semantics:** ARINC 424 Section `P`, Subsection `P` (Path Point records). Encodes SBAS/WAAS approach guidance: reference path identifier, horizontal/vertical alarm limits, glidepath angle (GPA), threshold crossing height (TCH), and flight path alignment point (FPAP).
- **X-Plane Representation:** 
  - **Row 14 (LPV FAS):** `14 lat lon elev channel 0.0 bearing ident airport region runway LPV` (e.g. `14 28.635934028 -17.755792917 104 47264 0.0 359.019 R36-Z GCLA GC 36 LPV`)
  - **Row 16 (LPV Threshold):** `16 lat lon elev channel tch_ft angle_bearing ident airport region approach_id` (e.g. `16 28.617376389 -17.755430694 104 47264 49.2 300359.019 R36-Z GCLA GC 36 E36A`)
- **OpenAIRAC Current Status:** **IMPLEMENTED (v1.1.0+)**. Decoded from primary and continuation `PP` records in `openairac-ingest`, modeled in `CanonicalLpvFas` with temporal migration `v11_lpv_fas.sql`, and serialized to `earth_nav.dat` rows 14 & 16 with sub-millidegree precision (100% channel and app type agreement on 4,709 golden approaches).
- **Importance for 1.0:** **High / Essential for GPS/WAAS Approaches**.
- **Source Available from Open Data:** **Yes (100% in FAA CIFP `PP` records)**.
- **Action Recommendation:** **SHIPPED**.

---

### 2.2. Enroute & Terminal Holdings (`earth_hold.dat`)
- **Source Semantics:** ARINC 424 Section `E`, Subsection `P` (Enroute Holds) and Section `P`, Subsection `D`/`E`/`F`/`G` (Terminal/Approach Holds). Encodes holding fix, inbound course, turn direction (L/R), leg length/time, minimum/maximum altitudes, and maximum holding speed.
- **X-Plane Representation:** `HOLD1140` format: `FixIdent Region Airport FixType InboundCourse LegTime LegDist TurnDirection MinAlt MaxAlt MaxSpeed` (e.g. `AE701 DA DAAE 11 171.0 1.0 0.0 R 5580 14000 230`).
- **OpenAIRAC Current Status:** Modeled in `openairac-procedures` (`PathTerminator::HF`, `HA`, `HM`) and `FAACIFP18` Section `H ` (6,193 lines), but not exported as a standalone `earth_hold.dat` file.
- **Importance for 1.0:** **Medium**. FMS units parse procedural holds directly from `CIFP/<ICAO>.dat`; `earth_hold.dat` provides fallback holds for published enroute intersections.
- **Source Available from Open Data:** **Yes (100% in FAA CIFP Section `H` and procedure legs)**.
- **Action Recommendation:** **POST-1.0 / Candidate for v1.1**.

---

### 2.3. Minimum Sector Altitudes (`earth_msa.dat`)
- **Source Semantics:** ARINC 424 Section `P`, Subsection `S` (MSA / Sector Altitudes). Encodes 25 NM sector emergency clearance altitudes around an airport or terminal navaid.
- **X-Plane Representation:** `MSAXP1150` format: `SectorCount CenterIdent Region Airport CenterType [Sector1Bearing Sector1Alt Sector1Radius ...]` (e.g. `3 BSA DA DAAD M 270 076 25 090 053 25 000 000 0`).
- **OpenAIRAC Current Status:** Present in `FAACIFP18` (`P:S` with 6,045 lines); not yet modeled in canonical store.
- **Importance for 1.0:** **Low**. Used primarily for synthetic vision display and secondary moving map rendering. Does not affect autopilot guidance or procedure tracking.
- **Source Available from Open Data:** **Yes (FAA CIFP `PS` records)**.
- **Action Recommendation:** **POST-1.0**.

---

### 2.4. Marker Beacons (`earth_nav.dat` Rows 7, 8, 9)
- **Source Semantics:** Outer Marker (OM), Middle Marker (MM), Inner Marker (IM) transmitter locations and associated runways.
- **X-Plane Representation:**
  - **Row 7 (Outer Marker):** `7 lat lon elev 0 0 bearing ident airport region runway OM`
  - **Row 8 (Middle Marker):** `8 lat lon elev 0 0 bearing ident airport region runway MM`
  - **Row 9 (Inner Marker):** `9 lat lon elev 0 0 bearing ident airport region runway IM`
- **OpenAIRAC Current Status:** Present in `FAACIFP18` (`PM` records); currently skipped during navaid ingestion.
- **Importance for 1.0:** **Medium**. Audio marker beacons and cockpit annunciators rely on these rows during classic ILS approaches.
- **Source Available from Open Data:** **Yes (FAA CIFP `PM` records)**.
- **Action Recommendation:** **POST-1.0 / Candidate for v1.1**.

---

### 2.5. GBAS / GLS Ground Stations (`earth_nav.dat` Row 15)
- **Source Semantics:** Ground-Based Augmentation System (GBAS) differential transmitter stations and VHF data broadcast (VDB) frequencies for precision approach guidance.
- **X-Plane Representation:** `15 lat lon elev channel_5digit range_nm bearing ident airport region runway GLS` (e.g. `15 40.146083333 44.377138889 2921 20731 80 300089.653 G08A UDYZ UD 08 GLS`).
- **OpenAIRAC Current Status:** Not in FAA CIFP master file (FAA publishes GBAS stations in specialty NASR feeds).
- **Importance for 1.0:** **Low**. Only a small fraction of worldwide airports operate operational civil GLS stations.
- **Source Available from Open Data:** **Partial (Requires FAA NASR or Open-AIP integration)**.
- **Action Recommendation:** **RESEARCH / Post-1.0**.

---

### 2.6. Grid Minimum Off-Route Altitudes (`earth_mora.dat`)
- **Source Semantics:** 1° $\times$ 1° lat/lon terrain clearance grid altitudes (in hundreds of feet).
- **X-Plane Representation:** `MORAXP1150` grid matrix of 30 integer values per 30° latitude block.
- **OpenAIRAC Current Status:** Not modeled.
- **Importance for 1.0:** **Low**. Informational display only.
- **Source Available from Open Data:** **Derived from public digital elevation models (SRTM/COPERNICUS)**.
- **Action Recommendation:** **POST-1.0**.

---

### 2.7. Airport Operational Metadata (`earth_aptmeta.dat`)
- **Source Semantics:** Transition Altitude (TA), Transition Level (TL), default speed restrictions (e.g. 250 KT below 10,000 FT).
- **X-Plane Representation:** `AptXP1210` format: `Airport Region Lat Lon Elev Class SpeedLimit SpeedAlt TransAlt TransLevel` (e.g. `KSFO K2 37.619 -122.375 13 C 250 10000 18000 FL180`).
- **OpenAIRAC Current Status:** Transition altitude/level modeled in `CanonicalAirport`, but file export not staged.
- **Importance for 1.0:** **Medium**. Used by default X-Plane ATC and FMS VNAV descent profiling.
- **Source Available from Open Data:** **Yes (OurAirports + FAA CIFP `PA` records)**.
- **Action Recommendation:** **POST-1.0 / Candidate for v1.1**.

---

## 3. Summary & Roadmap Implementation Guidance

| Feature | File / Row | 1.0 Release Verdict | Complexity | Open Source Provider |
| :--- | :--- | :--- | :--- | :--- |
| **ILS LOC & GS Direct Decode** | `earth_nav.dat` Rows 4 & 6 | **SHIPPED (1.0)** | Low | FAA CIFP `PI` |
| **LPV FAS Guidance** | `earth_nav.dat` Rows 14 & 16 | **SHIPPED (v1.1.0+)** | Medium | FAA CIFP `PP` |
| **Procedural Holds** | `earth_hold.dat` | Post-1.0 (v1.1) | Low | FAA CIFP `H` |
| **Marker Beacons** | `earth_nav.dat` Rows 7, 8, 9 | Post-1.0 (v1.1) | Low | FAA CIFP `PM` |
| **Airport Meta / Transitions**| `earth_aptmeta.dat` | Post-1.0 (v1.1) | Low | OurAirports / FAA |
| **Minimum Sector Altitudes** | `earth_msa.dat` | Post-1.0 (v1.2) | Medium | FAA CIFP `PS` |
| **GLS / GBAS Stations** | `earth_nav.dat` Row 15 | Post-1.0 (v1.2) | Medium | Open-AIP / NASR |
| **Grid MORA Matrix** | `earth_mora.dat` | Post-1.0 (v1.2) | Low | DEM Calculation |
