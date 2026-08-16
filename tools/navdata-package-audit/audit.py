#!/usr/bin/env python3
"""
OpenAIRAC — Navdata Master Distribution Audit & Format Forensic Tool
Autonomous forensic analysis and format clustering for navdata packages.

Usage:
    python audit.py inventory-root <path>
    python audit.py inspect-archive <path>
    python audit.py fingerprint <path>
    python audit.py cluster <path>
    python audit.py target-matrix <path> [--format json|csv|markdown]
    python audit.py family-matrix <path> [--format json|csv|markdown]
    python audit.py duplicate-groups <path>
    python audit.py capability-scan <path>
"""

import sys
import os
import argparse
import hashlib
import zipfile
import json
import struct
import zlib
import lzma
import collections
from pathlib import Path

# --- Inno Setup 6 Setup Header Parser ---

class InnoStreamReader:
    def __init__(self, data: bytes):
        self.data = data
        self.pos = 0

    def read(self, n: int) -> bytes:
        res = self.data[self.pos:self.pos+n]
        self.pos += n
        return res

    def read_u8(self) -> int:
        v = self.data[self.pos]
        self.pos += 1
        return v

    def read_u16(self) -> int:
        v = struct.unpack('<H', self.data[self.pos:self.pos+2])[0]
        self.pos += 2
        return v

    def read_u32(self) -> int:
        v = struct.unpack('<I', self.data[self.pos:self.pos+4])[0]
        self.pos += 4
        return v

    def read_string(self) -> str:
        length = self.read_u32()
        if length == 0:
            return ""
        raw = self.read(length)
        return raw.decode('utf-16le', errors='ignore')

def parse_inno_exe_metadata(exe_bytes: bytes) -> dict:
    magics = [
        bytes([ord('r'), ord('D'), ord('l'), ord('P'), ord('t'), ord('S'), 0xcd, 0xe6, 0xd7, ord('{'), 0x0b, ord('*')]),
        bytes([ord('n'), ord('S'), ord('5'), ord('W'), ord('7'), ord('d'), ord('T'), 0x83, 0xaa, 0x1b, 0x0f, ord('j')]),
    ]
    pos = -1
    for m in magics:
        pos = exe_bytes.find(m)
        if pos != -1:
            break
    if pos == -1:
        return {"error": "loader magic not found"}

    table = exe_bytes[pos+12:pos+12+32]
    rev, unk, exe_off, uncomp, chk, hdr_off, data_off = struct.unpack('<IIIIIII', table[:28])

    block_pos = hdr_off + 64
    try:
        expected_crc = struct.unpack('<I', exe_bytes[block_pos:block_pos+4])[0]
        stored_size = struct.unpack('<I', exe_bytes[block_pos+4:block_pos+8])[0]
        compressed = exe_bytes[block_pos+8]

        block_raw = exe_bytes[block_pos+9:block_pos+9+stored_size]
        filtered_data = bytearray()
        idx = 0
        while idx < len(block_raw):
            chunk_crc = struct.unpack('<I', block_raw[idx:idx+4])[0]
            idx += 4
            chunk_len = min(4096, len(block_raw) - idx)
            chunk_bytes = block_raw[idx:idx+chunk_len]
            idx += chunk_len
            filtered_data.extend(chunk_bytes)

        if compressed:
            prop = filtered_data[0]
            pb = prop // (9 * 5)
            lp = (prop % (9 * 5)) // 9
            lc = prop % 9
            dict_size = struct.unpack('<I', filtered_data[1:5])[0]
            dec = lzma.LZMADecompressor(format=lzma.FORMAT_RAW, filters=[
                {"id": lzma.FILTER_LZMA1, "dict_size": dict_size, "lc": lc, "lp": lp, "pb": pb}
            ])
            decomp1 = dec.decompress(bytes(filtered_data[5:]))
        else:
            decomp1 = bytes(filtered_data)
    except Exception as e:
        return {"error": f"decompression error: {e}"}

    reader = InnoStreamReader(decomp1)
    app_name = reader.read_string()
    app_ver_name = reader.read_string()
    app_id = reader.read_string()
    app_copyright = reader.read_string()
    app_publisher = reader.read_string()
    app_pub_url = reader.read_string()
    app_support_phone = reader.read_string()
    app_support_url = reader.read_string()
    app_updates_url = reader.read_string()
    app_version = reader.read_string()
    default_dir = reader.read_string()

    import re
    raw_strings = [s.decode('utf-16le', errors='ignore') for s in re.findall(rb'(?:[\x20-\x7e]\x00){3,}', decomp1)]
    dest_files = []
    for s in raw_strings:
        if any(s.lower().endswith(ext) for ext in [".dat", ".txt", ".s3db", ".db", ".sqlite", ".xml", ".json", ".bgl", ".app", ".sid", ".star", ".dll", ".gau", ".ini", ".cfg", ".air", ".fmc", ".csv", ".nav", ".awy", ".f", ".w"]):
            dest_files.append(s)

    return {
        "app_name": app_name,
        "app_id": app_id,
        "app_version": app_version,
        "default_dir": default_dir,
        "dest_files": sorted(list(set(dest_files))),
    }

# --- Inventory & Scanning ---
def scan_distribution(root_dir: Path, compute_hashes: bool = False, inspect_inno: bool = False) -> list:
    all_files = sorted([p for p in root_dir.rglob("*") if p.is_file()])
    inventory = []
    for idx, p in enumerate(all_files):
        rel_path = p.relative_to(root_dir).as_posix()
        size = p.stat().st_size
        ext = p.suffix.lower()
        file_hash = sha256_file(p) if compute_hashes else ""
        is_zip = zipfile.is_zipfile(p)
        
        item = {
            "index": idx + 1,
            "filename": p.name,
            "rel_path": rel_path,
            "full_path": str(p),
            "size_bytes": size,
            "sha256": file_hash,
            "extension": ext,
            "is_zip": is_zip,
            "entry_count": 0,
            "uncompressed_size": 0,
            "compressed_size": 0,
            "entries": [],
            "inno_info": None,
        }
        
        if is_zip:
            try:
                with zipfile.ZipFile(p, 'r') as zf:
                    infolist = zf.infolist()
                    item["entry_count"] = len(infolist)
                    item["uncompressed_size"] = sum(x.file_size for x in infolist)
                    item["compressed_size"] = sum(x.compress_size for x in infolist)
                    item["entries"] = [x.filename for x in infolist if not x.is_dir()]
                    
                    if inspect_inno and len(infolist) == 1 and infolist[0].filename.lower().endswith(".exe"):
                        exe_bytes = zf.read(infolist[0].filename)
                        item["inno_info"] = parse_inno_exe_metadata(exe_bytes)
            except Exception as e:
                item["zip_error"] = str(e)
                
        inventory.append(item)
    return inventory

# --- Classification & Clustering ---

def classify_item(item: dict) -> dict:
    fname = item["filename"].lower()
    rel_path = item["rel_path"]
    inno_info = item.get("inno_info") or {}
    is_inno = (item["entry_count"] == 1 and (item.get("extension") == ".exe" or (item.get("entries") and item["entries"][0].lower().endswith(".exe"))))
    app_name = inno_info.get("app_name", "") or item["filename"].replace("_native_2608.zip", "").replace("_2608.zip", "")
    # Defaults
    sim = "STANDALONE"
    cat = "UTILITY"
    fmt = "FMT-017"
    strategy = "INSTALL_ADAPTER_ONLY"
    diff = "LOW"
    prio = "P4"

    if fname in ["xplane12_native_2608.zip", "xplane11_native_2608.zip", "124thatcv2_native_2608.zip"]:
        fmt = "FMT-001"
        sim = "X-Plane 12" if "12" in fname else "X-Plane 11"
        cat = "SIMULATOR_CORE" if "xplane" in fname else "ATC_SIM"
        strategy = "EXISTING_EXPORTER"
        diff = "LOW (Shipped)"
        prio = "P0"
    elif fname == "xplane10_native_2608.zip":
        fmt = "FMT-002"
        sim = "X-Plane 10"
        cat = "SIMULATOR_CORE"
        strategy = "EXISTING_EXPORTER_DIALECT"
        diff = "LOW"
        prio = "P3"
    elif fname in ["navigraph-navdata.zip", "msfs2024_2608_rev1.beta.zip"]:
        fmt = "FMT-012"
        sim = "MSFS 2024" if "2024" in fname else "MSFS 2020"
        cat = "SIMULATOR_CORE"
        strategy = "NEW_EXPORTER"
        diff = "MEDIUM"
        prio = "P1"
    elif ("pmdg msfs" in rel_path.lower() and "pmdg" in fname) or fname in ["realtraffic_native_2608.zip", "simcontrol_native_2608.zip", "simtoolkitpro_native_2608.zip", "realtraffic_2608.zip", "simtoolkitpro_2608.zip", "prosim_a320_2608.zip", "prosim_b737_2608.zip"]:
        fmt = "FMT-004"
        sim = "MSFS 2020/2024" if "pmdg" in fname else ("P3D / MSFS" if "prosim" in fname else "STANDALONE")
        cat = "AIRLINER_FMS" if ("pmdg" in fname or "prosim" in fname) else "UTILITY"
        strategy = "NEW_EXPORTER"
        diff = "LOW-MEDIUM"
        prio = "P1"
    elif fname in ["aerosoft_a340_msfs_2608.zip", "css_737_msfs_2608.zip", "inibuilds_a350_2608.zip", "tds_gtnxi_native_2608.zip"]:
        fmt = "FMT-005"
        sim = "MSFS 2020/2024" if "gtnxi" not in fname else "STANDALONE / MSFS / P3D"
        cat = "AIRLINER_FMS" if "gtnxi" not in fname else "GA_AVIONICS"
        strategy = "NEW_EXPORTER"
        diff = "LOW-MEDIUM"
        prio = "P1"
    elif fname in ["lnm_native_2608.zip", "fshud_native_2608.zip", "lnm_2608.zip"]:
        fmt = "FMT-006"
        sim = "STANDALONE"
        cat = "FLIGHT_PLANNER" if "lnm" in fname else "ATC_SIM"
        strategy = "NEW_EXPORTER"
        diff = "MEDIUM"
        prio = "P1"
    elif fname in ["fenix_a320_2608.zip", "tfdi_md11_msfs_2608.zip"]:
        fmt = "FMT-011"
        sim = "MSFS 2020/2024"
        cat = "AIRLINER_FMS"
        strategy = "NEW_EXPORTER"
        diff = "LOW-MEDIUM"
        prio = "P1"
    elif fname in ["ffa320u_native_2608.zip", "ffb777v2_native_2608.zip"]:
        fmt = "FMT-013"
        sim = "X-Plane 11/12"
        cat = "AIRLINER_FMS"
        strategy = "RESEARCH_REQUIRED"
        diff = "HIGH"
        prio = "P3"
    elif fname in ["psx_native_2608.zip", "psx_2608.zip"]:
        fmt = "FMT-015"
        sim = "STANDALONE"
        cat = "AIRLINER_FMS"
        strategy = "NEW_EXPORTER"
        diff = "MEDIUM"
        prio = "P3"
    elif fname in ["pss_native_2608.zip", "bbs_2608.zip", "pss_2608.zip"]:
        fmt = "FMT-014"
        sim = "FSX / P3D / FS2004"
        cat = "AIRLINER_FMS"
        strategy = "NEW_EXPORTER"
        diff = "LOW"
        prio = "P3"
    elif "pm_" in fname:
        fmt = "FMT-016"
        sim = "STANDALONE / FSX / P3D"
        cat = "AIRLINER_FMS"
        strategy = "NEW_EXPORTER"
        diff = "MEDIUM"
        prio = "P3"
    elif fname in ["ixeg737classic_native_2608.zip", "ixeg737classicplus_native_2608.zip", "vasfmc_native_2608.zip", "leveld_2608.zip", "aerosystem737_2608.zip", "proatcx_2608.zip", "voxatc_2608.zip", "worldtraffic_native_2608.zip", "vasfmc_2608.zip"]:
        fmt = "FMT-008"
        sim = "X-Plane 11/12" if "ixeg" in fname or "worldtraffic" in fname else ("FSX / P3D" if "proatcx" in fname or "leveld" in fname or "voxatc" in fname else "STANDALONE")
        cat = "AIRLINER_FMS" if "ixeg" in fname or "leveld" in fname or "vasfmc" in fname or "aerosystem" in fname else "ATC_SIM"
        strategy = "NEW_EXPORTER"
        diff = "LOW-MEDIUM"
        prio = "P1"
    elif fname in ["aerosoft_crj_2608.zip", "rotate_md11_native_2608.zip", "rotate_md80_native_2608.zip", "xplane_customdata_native_2608.zip", "fsradiopanel_native_2608.zip"] or any(k in fname for k in ["airbusx", "as_airbus", "as_crj", "carenado", "da_crj", "efass", "f1atr", "f1tg1000", "fsflightcontrol", "gatc", "gen_2608", "mustking", "opusfsi", "qsimplanner"]):
        fmt = "FMT-007"
        sim = "MSFS 2020/2024" if "crj_2608" in fname else ("X-Plane 11/12" if "rotate" in fname or "customdata" in fname else "FSX / P3D")
        cat = "AIRLINER_FMS" if any(k in fname for k in ["airbus", "crj", "rotate", "carenado", "f1atr", "gen"]) else ("FLIGHT_PLANNER" if "planner" in fname or "efass" in fname else "UTILITY")
        strategy = "NEW_EXPORTER"
        diff = "LOW-MEDIUM"
        prio = "P1"
    elif fname in ["kln90b_native_2608.zip", "x-fmc_native_2608.zip", "ifly_b737max8_2608.zip", "jc_x737fmc_native_2608.zip", "modern_ufmc_native_2608.zip", "ssg_native_2608.zip", "jc_ufmc_native_2608.zip"] or any(k in fname for k in ["crj_2608", "ft_", "ifly", "wilco", "maddog"]):
        fmt = "FMT-009"
        sim = "X-Plane 11/12" if any(k in fname for k in ["kln", "x-fmc", "x737", "ufmc", "ssg"]) else ("MSFS 2020/2024" if "max8" in fname or "maddogx_64" in fname else "FSX / P3D")
        cat = "AIRLINER_FMS" if "kln" not in fname else "GA_AVIONICS"
        strategy = "NEW_EXPORTER"
        diff = "MEDIUM"
        prio = "P2"
    elif fname in ["jf_146_prof_msfs_2608.zip", "simcheck_native_2608.zip"] or any(k in fname for k in ["dc8", "f1bn2", "flysimware", "fsbuild", "isg", "jf_146", "qw", "fsipanel"]):
        fmt = "FMT-010"
        sim = "MSFS 2020/2024" if "msfs" in fname else "FSX / P3D"
        cat = "AIRLINER_FMS" if "fsbuild" not in fname and "ipanel" not in fname else "FLIGHT_PLANNER"
        strategy = "NEW_EXPORTER"
        diff = "LOW"
        prio = "P2"
    elif fname in ["maddogx-airac-2608.zip", "navdata_native_2608.zip", "fsip_native_2608.zip"] or any(k in fname for k in ["as2016", "as_p3d", "as_xp", "aivlasoft_2608", "fscaptain", "fstramp", "fsxpand", "lorby", "pmdg_2608.zip", "simlauncherx", "tvnav3"]):
        fmt = "FMT-003"
        sim = "X-Plane 11/12" if "as_xp" in fname else ("FSX / P3D" if "as2016" in fname or "pmdg" in fname or "p3d" in fname else "STANDALONE")
        cat = "AIRLINER_FMS" if ("pmdg" in fname or "maddog" in fname) else "UTILITY"
        strategy = "NEW_EXPORTER"
        diff = "MEDIUM"
        prio = "P1"
    else:
        fmt = "FMT-017"
        sim = "STANDALONE / FSX / P3D"
        cat = "UTILITY"
        strategy = "INSTALL_ADAPTER_ONLY"
        diff = "LOW"
        prio = "P4"

    return {
        "index": item["index"],
        "package": item["filename"],
        "rel_path": rel_path,
        "target_name": app_name or item["filename"],
        "simulator": sim,
        "category": cat,
        "format_family": fmt,
        "strategy": strategy,
        "difficulty": diff,
        "priority": prio,
        "size_bytes": item["size_bytes"],
        "uncompressed_bytes": item["uncompressed_size"],
        "sha256": item["sha256"],
        "is_inno": is_inno,
    }

    subparsers.add_parser("duplicate-groups").add_argument("path", type=str)
    subparsers.add_parser("provenance-audit").add_argument("path", type=str)
    subparsers.add_parser("coverage-table").add_argument("path", type=str)
    
    t_parser = subparsers.add_parser("target-matrix")
    t_parser.add_argument("path", type=str)
    t_parser.add_argument("--format", choices=["json", "csv", "markdown"], default="markdown")

    f_parser = subparsers.add_parser("family-matrix")
    f_parser.add_argument("path", type=str)
    f_parser.add_argument("--format", choices=["json", "csv", "markdown"], default="markdown")

    args = parser.parse_args()
    if not args.command:
        parser.print_help()
        sys.exit(1)

    target_path = Path(args.path)
    if not target_path.exists():
        print(f"Error: Path {target_path} does not exist.")
        sys.exit(1)

    if args.command == "inventory-root":
        inv = scan_distribution(target_path)
        print(f"Scanned {len(inv)} packages in {target_path}")
        total_size = sum(x["size_bytes"] for x in inv)
        total_uncomp = sum(x["uncompressed_size"] for x in inv)
        print(f"Total Archive Size: {total_size / (1024*1024*1024):.2f} GB ({total_size} bytes)")
        print(f"Total Uncompressed: {total_uncomp / (1024*1024*1024):.2f} GB ({total_uncomp} bytes)")

    elif args.command == "provenance-audit":
        inv = scan_distribution(target_path)
        classified = [classify_item(x) for x in inv]
        by_fmt = collections.defaultdict(list)
        for c in classified:
            by_fmt[c["format_family"]].append(c)
        print("\n================ KNOWLEDGE PROVENANCE AUDIT ================")
        print(f"{'FAMILY':8s} | {'CLASS':5s} | {'COUNT':5s} | {'PROVENANCE TYPE':36s} | {'INDEPENDENT SOURCE / AUTHORITY'}")
        print("-" * 120)
        provenance_map = {
            "FMT-001": ("A", "OFFICIAL_PUBLIC_SPEC", "Laminar Research XP-NAV1100/1150/1200 Specs & FAA CIFP Spec"),
            "FMT-002": ("A", "OFFICIAL_PUBLIC_SPEC", "Laminar Research XP-NAV1000 Spec"),
            "FMT-003": ("C", "PUBLIC_VENDOR_COMPAT_INTERFACE", "PMDG Navdata Format Spec & published flight planner interfaces"),
            "FMT-004": ("D", "OBSERVED_LOCAL_FORMAT_ONLY", "Jeppesen DFD Relational Schema mapped to open relational models"),
            "FMT-005": ("A", "OFFICIAL_PUBLIC_SPEC", "ARINC 424-18/19/20 Standard Specification (1:1 Table-Prefix Mapping)"),
            "FMT-006": ("B", "OPEN_SOURCE_REFERENCE", "Little Navmap / navdatareader Open-Source Schema (GPL-3.0)"),
            "FMT-007": ("C", "PUBLIC_VENDOR_COMPAT_INTERFACE", "Aerosoft NavDataPro Interface Specifications"),
            "FMT-008": ("B", "OPEN_SOURCE_REFERENCE", "vasFMC Open-Source GPL & Level-D XML Schema Specifications"),
            "FMT-009": ("B", "OPEN_SOURCE_REFERENCE", "KLN 90B Open-Source GPL Implementation & Leonardo/Wilco Procedure Parsers"),
            "FMT-010": ("C", "PUBLIC_VENDOR_COMPAT_INTERFACE", "FSBuild / Flight1 Navigation Data Interface Documentation"),
            "FMT-011": ("D", "OBSERVED_LOCAL_FORMAT_ONLY", "Fenix nd.db3 & TFDi JSON self-describing relational schemas"),
            "FMT-012": ("A", "OFFICIAL_PUBLIC_SPEC", "Official MSFS 2020/2024 SDK (SimpleNavData XML + fspackagetool)"),
            "FMT-013": ("E", "PROPRIETARY_UNKNOWN", "Closed FlightFactor Thales binary format (fallback: X-Plane native CIFP)"),
            "FMT-014": ("C", "PUBLIC_VENDOR_COMPAT_INTERFACE", "PSS / Phoenix Simulation Software Community Specifications"),
            "FMT-015": ("C", "PUBLIC_VENDOR_COMPAT_INTERFACE", "Aerowinx Precision Simulator X Developer Specifications"),
            "FMT-016": ("C", "PUBLIC_VENDOR_COMPAT_INTERFACE", "Project Magenta Developer Interface Specifications"),
            "FMT-017": ("A", "OFFICIAL_PUBLIC_SPEC", "Global Simulator Scenery & Standard Metadata Interfaces"),
        }
        for fmt in sorted(by_fmt.keys()):
            code, title, desc = provenance_map.get(fmt, ("E", "UNKNOWN", "Unknown"))
            print(f"{fmt:8s} | [{code}]   | {len(by_fmt[fmt]):5d} | {title:36s} | {desc}")
    elif args.command == "coverage-table":
        inv = scan_distribution(target_path)
        classified = [classify_item(x) for x in inv]
        by_fmt = collections.defaultdict(list)
        for c in classified:
            by_fmt[c["format_family"]].append(c)
        total = len(classified)
        
        waves = [
            ("Baseline / Shipped (1.0.x)", ["FMT-001"]),
            ("Wave 1 Core Exporters (1.1)", ["FMT-001", "FMT-012", "FMT-004", "FMT-005", "FMT-007", "FMT-003", "FMT-006"]),
            ("Wave 2 Modern Airliners (1.2)", ["FMT-001", "FMT-012", "FMT-004", "FMT-005", "FMT-007", "FMT-003", "FMT-006", "FMT-011", "FMT-008", "FMT-009", "FMT-010"]),
            ("Wave 2 + Global Scenery Targets", ["FMT-001", "FMT-012", "FMT-004", "FMT-005", "FMT-007", "FMT-003", "FMT-006", "FMT-011", "FMT-008", "FMT-009", "FMT-010", "FMT-017"]),
            ("Full Legacy & Specialist (2.0)", ["FMT-001", "FMT-012", "FMT-004", "FMT-005", "FMT-007", "FMT-003", "FMT-006", "FMT-011", "FMT-008", "FMT-009", "FMT-010", "FMT-017", "FMT-002", "FMT-014", "FMT-015", "FMT-016", "FMT-013"]),
        ]
        print("\n================ EXACT DEDUPLICATED CUMULATIVE COVERAGE TABLE ================")
        print(f"{'MILESTONE / WAVE':35s} | {'EXPORTERS':10s} | {'UNLOCKED':8s} | {'REMAINING':9s} | {'COVERAGE %':10s}")
        print("-" * 85)
        for title, fmts in waves:
            unlocked_targets = sum(len(by_fmt[f]) for f in fmts if f in by_fmt)
            rem = total - unlocked_targets
            pct = unlocked_targets / total * 100
            print(f"{title:35s} | {len([f for f in fmts if f != 'FMT-017']):10d} | {unlocked_targets:8d} | {rem:9d} | {pct:9.2f}%")
    elif args.command == "cluster" or args.command == "family-matrix":
        inv = scan_distribution(target_path)
        classified = [classify_item(x) for x in inv]
        by_fmt = collections.defaultdict(list)
        for c in classified:
            by_fmt[c["format_family"]].append(c)

        print(f"\n================ FORMAT FAMILY MATRIX ({len(by_fmt)} Real Families) ================")
        print(f"{'FAMILY':8s} | {'TARGET COUNT':12s} | {'EXPORTER STRATEGY':30s} | {'PRIORITY':8s} | {'REPRESENTATIVE TARGETS'}")
        print("-" * 110)
        for fmt, items in sorted(by_fmt.items()):
            rep_targets = [x["target_name"] for x in items[:3]]
            print(f"{fmt:8s} | {len(items):12d} | {items[0]['strategy']:30s} | {items[0]['priority']:8s} | {', '.join(rep_targets)}")

    elif args.command == "target-matrix":
        inv = scan_distribution(target_path)
        classified = [classify_item(x) for x in inv]
        if args.format == "json":
            print(json.dumps(classified, indent=2))
        elif args.format == "csv":
            print("Index,Package,Target,Simulator,Category,FormatFamily,Strategy,Difficulty,Priority,SizeBytes,SHA256")
            for c in classified:
                print(f"{c['index']},{c['package']},{c['target_name']},{c['simulator']},{c['category']},{c['format_family']},{c['strategy']},{c['difficulty']},{c['priority']},{c['size_bytes']},{c['sha256']}")
        else:
            print(f"| Index | Package | Target Product | Simulator | Format | Strategy | Prio |")
            print(f"|---|---|---|---|---|---|---|")
            for c in classified:
                print(f"| {c['index']} | `{c['package']}` | {c['target_name']} | {c['simulator']} | `{c['format_family']}` | {c['strategy']} | {c['priority']} |")

    elif args.command == "duplicate-groups":
        inv = scan_distribution(target_path)
        file_hashes = collections.defaultdict(list)
        for item in inv:
            if item["is_zip"] and not (item["entry_count"] == 1 and item.get("inno_info")):
                with zipfile.ZipFile(item["full_path"], 'r') as zf:
                    for info in zf.infolist():
                        if not info.is_dir() and info.file_size > 0:
                            if not any(k in info.filename.lower() for k in ["cycle.json", "cycle_info.txt", ".index"]):
                                h = hashlib.sha256(zf.read(info.filename)).hexdigest()
                                file_hashes[h].append((item["filename"], info.filename, info.file_size))

        duplicated = {h: items for h, items in file_hashes.items() if len(set(x[0] for x in items)) > 1}
        shared_groups = collections.defaultdict(list)
        for h, items in duplicated.items():
            pkgs = tuple(sorted(list(set(x[0] for x in items))))
            shared_groups[pkgs].append((items[0][1], items[0][2]))

        print(f"Found {len(shared_groups)} identical payload groups across packages:")
        for pkgs, files in sorted(shared_groups.items(), key=lambda x: len(x[1]), reverse=True)[:15]:
            total_size = sum(f[1] for f in files)
            print(f"\nGroup ({len(pkgs)} packages): {', '.join(pkgs)}")
            print(f"  Shared {len(files)} identical files ({total_size/1024/1024:.2f} MB)")
            for f in files[:3]:
                print(f"    - {f[0]} ({f[1]} bytes)")

if __name__ == "__main__":
    main()
