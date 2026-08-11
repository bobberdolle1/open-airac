# 📖 OpenAIRAC Installation & Setup Guide

This guide explains how to install, configure, and use **OpenAIRAC** with **X-Plane 12** and **MSFS 2024**.

---

## 📥 Option 1: Pre-compiled ZIP (Recommended for Users)

1. Go to **[OpenAIRAC Releases](https://github.com/bobberdolle1/open-airac/releases)**.
2. Download `OpenAIRAC-v0.1.0-Windows-x64.zip`.
3. Unpack the ZIP archive to any folder (e.g., `C:\OpenAIRAC`).
4. Open **PowerShell** or **Command Prompt** in that directory.
5. Run the sync command pointing to your simulator installation:

### For X-Plane 12:
```powershell
.\openairac.exe sync --sim xp12 --path "F:\SteamLibrary\steamapps\common\X-Plane 12"
```

The tool will:
- Connect to open aeronautical data repositories (OurAirports / FAA).
- Compute dynamic WMM magnetic variation for all navaids and runways for the current year.
- Export native `earth_nav.dat` into `X-Plane 12/Custom Data/`.

---

## 🛠️ Option 2: Building from Source (Developer Setup)

### Prerequisites:
- [Rust Toolchain](https://www.rust-lang.org/) (1.85+)
- Git

### Build & Run:
```bash
# Clone the repository
git clone https://github.com/bobberdolle1/open-airac.git
cd open-airac

# Run tests
cargo test --workspace

# Build optimized binary
cargo build --release

# Execute sync
./target/release/openairac-cli sync --sim xp12 --path "C:/Program Files/X-Plane 12"
```

---

## 🧭 Calculating Dynamic Magnetic Variation (CLI Tool)

You can check Earth's magnetic variation for any coordinate and year using the WMM engine:

```powershell
.\openairac.exe magvar --lat 55.9726 --lon 37.4146 --year 2026.6
```

Output:
```text
🧭 Lat: 55.9726, Lon: 37.4146, Year: 2026.6
📍 Magnetic Declination (Variation): +11.8°
```
