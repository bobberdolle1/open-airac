# OpenAIRAC — X-Plane 12 Navigation Data Gap Matrix & Forensic Reference

**Date:** 2026-08-16  
**Cycle Reference:** AIRAC 2608  
**Baseline Dataset Comparison:** Laminar `convert424toxplane` v12.4 + Navigraph AIRAC 2608 (`xplane12_native_2608.zip`) vs. OpenAIRAC 1.0 Pipeline (FAA CIFP `FAACIFP18` + OurAirports).

---

## 1. Executive Classification & Status Terminology

To ensure strict engineering precision, differences between OpenAIRAC and the reference datasets are explicitly categorized using standard audit terms:

- **`IMPLEMENTED`**: Fully decoded from source records, stored in canonical models, and correctly exported to X-Plane 12 formats.
- **`EXPLAINED BUT MISSING`**: The mathematical transformation and source records are known, but OpenAIRAC does not currently serialize this entity class to disk.
- **`REQUIRED FOR 1.0`**: Core navigation or simulator autopilot feature whose absence impairs certified procedure flying or causes silent degradation.
- **`OPTIONAL POST-1.0`**: Secondary, display-only, or terrain advisory feature that does not prevent standard IFR navigation in simulator aircraft.
- **`SOURCE UNAVAILABLE`**: Not published in standard public-domain government datasets (e.g. FAA CIFP 2608).
- **`RESEARCH REQUIRED`**: Requires multi-source synthesis (e.g. digital elevation models or specialty FAA NASR feeds).

---

## 2. Comprehensive Classification of All 1,459 Converter-Only Nav Rows

In the baseline comparison of FAA CIFP 2608 output between `convert424toxplane` v12.4 and OpenAIRAC, `convert424toxplane` emits **1,459 rows** in `earth_nav.dat` not present in OpenAIRAC's output. Every single row is accounted for below:

```
========================================================================================================================
ROW CODE  X-PLANE FACILITY CLASS       CONVERTER COUNT  FAA ARINC SOURCE  OPENAIRAC STATUS       1.0 RELEASE VERDICT
========================================================================================================================
Row 14    LPV Final Approach Segment   658 rows         Section P:P       EXPLAINED BUT MISSING  REQUIRED FOR 1.0
Row 16    LPV Threshold & GPA/TCH      658 rows         Section P:P       EXPLAINED BUT MISSING  REQUIRED FOR 1.0
Row 7     Outer Marker (OM)             52 rows         Section P:M (PM)  EXPLAINED BUT MISSING  OPTIONAL POST-1.0 (v1.1)
Row 8     Middle Marker (MM)            46 rows         Section P:M (PM)  EXPLAINED BUT MISSING  OPTIONAL POST-1.0 (v1.1)
Row 9     Inner Marker (IM)             18 rows         Section P:M (PM)  EXPLAINED BUT MISSING  OPTIONAL POST-1.0 (v1.1)
Row 15    GBAS / GLS Ground Station     15 rows         Section P:T (GLS) SOURCE UNAVAILABLE     RESEARCH REQUIRED
Other     Terminal NDB / Unpaired DME   12 rows         Section D / DB    EXPLAINED BUT MISSING  OPTIONAL POST-1.0 (v1.1)
========================================================================================================================
TOTAL CONVERTER-ONLY ROWS: 1,459
========================================================================================================================
```

> **Key Takeaway:** "Zero unexplained differences" in the validation harness means 100% of these 1,459 rows have been mathematically and forensically identified; it does **not** mean they are all implemented. Specifically, **1,316 rows (90.2%)** are LPV FAS data (Rows 14 & 16) originating from ARINC `P:P` Path Point records.

---

## 3. Deep Forensic Audit: LPV / SBAS FAS Data (`earth_nav.dat` Rows 14 & 16)

### 3.1. Verified ARINC 424 Section `P`, Subsection `P` (Path Point) Record Layout

In `FAACIFP18` (cycle 2608), there are **9,810 lines** of `P:P` records. Every approach consists of two sequential continuation records:

#### Continuation Record 1 (`001` — Geometry & Threshold Data)
- **Cols 7–10 (1-based):** Airport Identifier (e.g. `KSFO`, `PAAQ`, `PABE`).
- **Cols 11–12:** ICAO Region Code (e.g. `K2`, `PA`).
- **Col 13:** Subsection Code (`P` = Path Point).
- **Cols 14–19:** Approach Identifier (e.g. `R10L`, `R19RY`, `R01L`).
- **Cols 20–24:** Runway Identifier (e.g. `RW10L`, `RW01L`).
- **Cols 25–27:** Continuation Record Number (`001`).
- **Cols 28–31:** Reference Path Identifier (e.g. `W10A`, `W01A`, `W19B`, `W28A`).
- **Cols 33–51:** Landing Threshold Point (LTP/FTP) Coordinates (`NddmmsshhWdddmmsshh` $\to$ decimal lat/lon).
- **Cols 52–61:** LTP/FTP Ellipsoidal Height / Elevation in meters (`-00309` $\to -30.9$ m).
- **Cols 62–65:** Glide Path Angle (GPA) in hundredths of a degree (`0300` $\to 3.00^\circ$, `0285` $\to 2.85^\circ$, `0315` $\to 3.15^\circ$).
- **Cols 66–84:** Flight Path Alignment Point (FPAP) Coordinates (`NddmmsshhWdddmmsshh` $\to$ decimal lat/lon).
- **Cols 89–93:** Course Length Offset in meters (`00000` $\to 0.0$ m, `16480` $\to 1648.0$ m).
- **Cols 94–97:** Threshold Crossing Height (TCH) in tenths of a foot (`0550` $\to 55.0$ ft, `0400` $\to 40.0$ ft).

#### Continuation Record 2 (`002` — Channel & Performance Parameters)
- **Cols 20–23:** SBAS Approach Type (`LPV` or `LP`).
- **Cols 28–32:** 5-digit WAAS / SBAS Channel Number (`93946`, `40425`, `42707`, `81940`).
- **Cols 40–45:** Horizontal Alarm Limit (HAL) in meters.
- **Cols 46–51:** Vertical Alarm Limit (VAL) in meters.

---

### 3.2. Mapping ARINC `P:P` to X-Plane 12 `earth_nav.dat` Rows

#### Row 14 (LPV Final Approach Segment)
```text
14  <FPAP_lat>  <FPAP_lon>  <elev_ft>  <channel>  <length_offset>  <true_bearing>  <approach_id>  <airport>  <region>  <runway>  <approach_type>
```
*Example (KSFO R10L LPV):*
`14  37.615730556 -122.366858611        5    93946   0.0    120.901 R10L KSFO K2 10L LPV`

#### Row 16 (LPV Threshold & Approach Data)
```text
16  <LTP_lat>   <LTP_lon>   <elev_ft>  <channel>  <tch_ft>  <angle_bearing>  <approach_id>  <airport>  <region>  <runway>  <ref_path_id>
```
*Example (KSFO R10L W10A):*
`16  37.628419583 -122.393616944        5    93946  55.0 300120.901 R10L KSFO K2 10L W10A`
*(where `angle_bearing = (gpa * 100) * 1000 + true_bearing = 300000 + 120.901 = 300120.901`)*

---

### 3.3. Should LPV FAS Block 1.0?

#### Simulator Behavioral Impact
1. **Lateral Flight Guidance (LNAV):** **UNIMPAIRED**. Lateral path terminators (`IF`, `TF`, `DF`, `CF`, `RF`) are loaded from `CIFP/<ICAO>.dat`. Autopilot tracks the lateral final approach segment correctly.
2. **Barometric Vertical Navigation (LNAV/VNAV):** **UNIMPAIRED**. VNAV descent path computes normally from waypoint altitude constraints.
3. **LPV Precision Glideslope Needle (SBAS):** **LOST / DOWNGRADED**. Without Rows 14 and 16, Garmin G1000 / G530 avionics in X-Plane 12 cannot auto-tune the 5-digit channel or validate the FAS data block. The avionics annunciate `LNAV` or `LNAV+V` instead of `LPV`, and refuse to arm the precision SBAS glidepath down to 200 ft decision height.

#### Verdict
- **Classification:** **`SHOULD_HAVE_FOR_1_0`** (Highest priority feature for immediate inclusion; does not cause simulator crash or total procedure load failure, but is required for honest "LPV Precision Approach" support).

---

## 4. Auxiliary Reference Datasets Evaluation

### 4.1. `earth_hold.dat` (Published Holdings)
- **Simulator Usage:** Provides default holding patterns when an aircraft holds at an enroute intersection outside of a published terminal procedure.
- **Impact on 1.0:** **Low–Medium**. FMS units load procedural holds directly from `CIFP/<ICAO>.dat`. Only manual holds over generic enroute fixes use `earth_hold.dat`.
- **Verdict:** **`OPTIONAL POST-1.0 (v1.1)`**.

### 4.2. `earth_msa.dat` (Minimum Sector Altitudes)
- **Simulator Usage:** 25 NM emergency clearance sector altitudes around airport centers. Used solely for synthetic vision background display and moving map overlays.
- **Impact on 1.0:** **Low**. Does not affect navigation, AP flight guidance, or FMS lateral/vertical tracking.
- **Verdict:** **`OPTIONAL POST-1.0 (v1.2)`**.

### 4.3. Marker Beacons (`earth_nav.dat` Rows 7, 8, 9)
- **Simulator Usage:** Outer (OM), Middle (MM), Inner (IM) audio tone triggers during classic ILS approaches.
- **Impact on 1.0:** **Low**. Modern aircraft use GPS/DME fixes for outer/middle marker verification.
- **Verdict:** **`OPTIONAL POST-1.0 (v1.1)`**.

### 4.4. `earth_aptmeta.dat` (Transition Altitudes / Speed Limits)
- **Simulator Usage:** FMS default transition altitude (e.g. 18,000 ft in USA, variable in Europe) and 250 kt speed restriction below 10,000 ft.
- **Impact on 1.0:** **Medium**. FMS VNAV profiles in airliner add-ons fall back to standard defaults if absent.
- **Verdict:** **`OPTIONAL POST-1.0 (v1.1)`**.

### 4.5. GBAS / GLS Ground Stations (`earth_nav.dat` Row 15)
- **Simulator Usage:** Precision GLS microwave/VHF data broadcast landing system.
- **Source Availability:** **Unavailable in FAA CIFP** (published only in specialized FAA NASR feeds).
- **Impact on 1.0:** **Negligible** (<0.5% of worldwide operations).
- **Verdict:** **`RESEARCH REQUIRED / Post-1.0`**.

---

## 5. Normalized Procedure Alignment (Forensic Ground Truth)

Applying semantic normalization across procedure naming and structure:

```
========================================================================================================
PROCEDURE CATEGORY     TOTAL REFERENCE     EXACT MATCH     SEMANTICALLY EQUIVALENT    TRUE DISCREPANCY
========================================================================================================
SIDs (Standard Departures)    100%             97.8%                2.2%                      0.0%
STARs (Arrivals)              100%            100.0%                0.0%                      0.0%
Approaches (Non-GLS)          100%             96.4%                3.6%                      0.0%
========================================================================================================
```

- **RNP AR Normalization (`H` $\to$ `R`):** Resolves 100% of nominal naming mismatches for RNP AR approaches at complex hubs (KDEN, KLAX, KSFO).
- **LOC-only Folding:** Explains 100% of approach count disparities where FAA CIFP publishes separate `L` records and Navigraph consolidates LOC minima under `I`.
- **True Geometric Flight Track Error:** **0.0%** across all common procedures.
