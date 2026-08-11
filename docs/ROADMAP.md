# 🗺️ OpenAIRAC Roadmap (P0 - P7)

---

### Phase 0 — Foundation & Integrity Refactor (COMPLETE / IN PROGRESS)
- [x] Integrate genuine **NOAA WMM2025** spherical harmonic expansion algorithm.
- [x] Verify WMM2025 implementation against official NOAA reference test vectors.
- [x] Implement dual runway magnetic drift detector (`official_designator` vs `computed_magnetic_designator`).
- [x] Refactor Rust workspace into modular crates (`magnetic`, `nav-model`, `openairac-core`, `openairac-exporter`, `openairac-plugin`, `openairac-cli`).
- [x] Establish data provenance & temporal validity structs.
- [x] Fix Mermaid diagram syntax and release pipeline.

---

### Phase 1 — OpenAIRAC Canonical Temporal Database (P1)
- [ ] Implement SQLite + R*Tree storage engine (`world.openairac.sqlite`).
- [ ] Support temporal range queries (`valid_from` / `valid_until`) to query navigation world at any historical date.
- [ ] Build automated delta update & revision tracking engine.

---

### Phase 2 — FAA CIFP & ARINC 424 Procedure Engine (P2)
- [ ] Integrate `arinc424` Rust crate v0.4.0 for parsing 132-byte FAA CIFP records.
- [ ] Implement ARINC 424 Leg Interpreter (`IF`, `TF`, `CF`, `DF`, `RF` arcs, `VA`, `VI`, `HM`, `HF`, `HA`).

---

### Phase 3 — Complete X-Plane 12 NavData Integration (P3)
- [ ] Full exporter for `earth_fix.dat`, `earth_nav.dat`, `earth_awy.dat`, `CIFP/*.dat`.
- [ ] Support custom airport procedure bundling.

---

### Phase 4 — Route Planner Engine (P4)
- [ ] Airway graph construction & contraction hierarchy routing solver.
- [ ] Constraints: altitude capability, RNAV restrictions, direction, penalties.

---

### Phase 5 — MSFS 2024 SDK & Packaging (P5)
- [ ] MSFS 2024 BGL / NavData XML package generation.

---

### Phase 6 — OpenAIRAC Flight Deck EFB (P6)
- [ ] Tauri 2.0 + React 19 + TypeScript + MapLibre GL JS vector EFB & moving map.
- [ ] Real-time flight plan sync across aircraft FMS, sim, and EFB.

---

### Phase 7 — Visual Procedure Studio (P7)
- [ ] Visual SID/STAR/Approach editor for virtual airlines and custom scenery creators.
