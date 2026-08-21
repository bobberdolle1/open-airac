#!/usr/bin/env python3
"""
OpenAIRAC 3.2 — AI Crew Gateway Developer Example.

Demonstrates deterministic interaction between an AI flight crew consumer
and OpenAIRAC Map / Core running on localhost.

Navigation truth stays inside OpenAIRAC. AI reasoning stays above.
"""

import json
import sys
import urllib.request
import urllib.error

DEFAULT_BASE_URL = "http://127.0.0.1:8989/api/openairac/v1"


def query_api(endpoint: str, base_url: str = DEFAULT_BASE_URL) -> dict:
    url = f"{base_url}{endpoint}"
    req = urllib.request.Request(url, headers={"Accept": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=5) as response:
            return json.loads(response.read().decode("utf-8"))
    except urllib.error.URLError as e:
        return {"error": "CONNECTION_FAILED", "detail": str(e), "url": url}


def ask_ai_crew(question: str, snapshot: dict) -> dict:
    """Deterministic question answering engine powered by OpenAIRAC Flightdeck Snapshot."""
    q = question.lower()
    
    if "where are we" in q or "position" in q or "location" in q:
        phase = snapshot.get("flight_phase", "UNKNOWN")
        pos = snapshot.get("position")
        geom = snapshot.get("navigation_geometry", {})
        if pos:
            lat = pos.get("latitude_deg", 0.0)
            lon = pos.get("longitude_deg", 0.0)
            alt = pos.get("altitude_msl_ft", 0.0)
            gs = pos.get("groundspeed_kts", 0.0)
            trk = pos.get("track_true_deg", 0.0)
            text = f"We are currently in {phase} phase at FL{alt/100:.0f} ({alt:.0f} ft MSL), groundspeed {gs:.0f} kt, tracking {trk:.0f}°."
        else:
            text = f"We are in {phase} phase. {snapshot.get('phase_evidence', '')}"
        
        xtk_nm = geom.get("xtk_nm", 0.0)
        side = geom.get("xtk_side", "ON_TRACK")
        off_route = geom.get("is_off_route", False)
        route_status = "OFF ROUTE" if off_route else "ON ROUTE"
        
        return {
            "question": question,
            "answer": text,
            "route_tracking": f"XTK: {xtk_nm:.2f} NM {side} ({route_status})",
            "evidence": {
                "flight_phase": phase,
                "position": pos,
                "navigation_geometry": geom
            }
        }
        
    elif "star" in q or "approach" in q or "arrival" in q:
        dest = snapshot.get("destination", {})
        if dest.get("is_source_required"):
            answer = f"Destination {dest.get('ident')} requires official AIP source dataset (SOURCE_REQUIRED). No terminal procedures (STAR/Approach) are published in open data."
        else:
            star = dest.get("procedure_name", "DIRECT")
            rwy = dest.get("selected_runway", "DEFAULT")
            answer = f"Expecting STAR {star} for Runway {rwy} at {dest.get('ident')}."
            
        return {
            "question": question,
            "answer": answer,
            "evidence": dest
        }
        
    elif "tod" in q or "descend" in q or "descent" in q:
        descent = snapshot.get("descent_profile", {})
        tod_dist = descent.get("tod_distance_nm")
        status = descent.get("profile_status", "UNKNOWN")
        req_vs = descent.get("required_descent_rate_fpm")
        dev = descent.get("profile_deviation_ft")
        
        if tod_dist is not None:
            answer = f"Top of Descent is in {tod_dist:.1f} NM. Status: {status}."
        elif status == "PASSED" or snapshot.get("flight_phase") == "DESCENT":
            req_str = f"{req_vs:.0f} fpm" if req_vs is not None else "--"
            dev_str = f"{dev:+.0f} ft" if dev is not None else "--"
            answer = f"Descent is in progress ({status}). Required vertical speed: {req_str}, profile deviation: {dev_str}."
        else:
            answer = f"Descent profile status: {status}."
            
        return {
            "question": question,
            "answer": answer,
            "evidence": descent
        }
        
    elif "next" in q or "leg" in q or "flying" in q:
        active_leg = snapshot.get("active_leg", {})
        geom = snapshot.get("navigation_geometry", {})
        dist_next = geom.get("distance_to_next_fix_nm", 0.0)
        ete_sec = geom.get("ete_next_fix_sec")
        ete_str = f"{ete_sec // 60}m {ete_sec % 60}s" if ete_sec is not None else "--:--"
        next_fix = active_leg.get("next_fix", "DEST")
        leg_name = active_leg.get("leg_name", "DIRECT")
        
        answer = f"We are flying leg {leg_name}. Next waypoint is {next_fix} in {dist_next:.1f} NM (ETE: {ete_str})."
        return {
            "question": question,
            "answer": answer,
            "evidence": {
                "active_leg": active_leg,
                "distance_to_next_nm": dist_next,
                "ete_next_sec": ete_sec
            }
        }
        
    elif "weather" in q or "metar" in q or "wind" in q:
        wx = snapshot.get("weather_summary", {})
        dest_metar = wx.get("destination_metar", "Unavailable")
        dest_rw = wx.get("destination_runway_wind", {})
        dest_icao = snapshot.get("destination", {}).get("ident", "DEST")
        
        answer = f"Destination weather for {dest_icao}: {dest_metar}."
        if dest_rw:
            answer += f" Runway {dest_rw.get('runway_ident')}: Headwind {dest_rw.get('headwind_kts', 0):.0f} kt, Crosswind {dest_rw.get('crosswind_kts', 0):.0f} kt."
            
        return {
            "question": question,
            "answer": answer,
            "evidence": wx
        }
        
    elif "current" in q or "fresh" in q or "stale" in q:
        freshness = snapshot.get("freshness", {})
        if isinstance(freshness, dict):
            telem_status = freshness.get("telemetry", "UNKNOWN")
            wx_status = freshness.get("weather", "UNKNOWN")
            online_status = freshness.get("online", "UNKNOWN")
            nav_status = freshness.get("navdata", "UNKNOWN")
            age_ms = freshness.get("telemetry_age_ms", 0)
            answer = f"Data freshness: Telemetry is {telem_status} ({age_ms} ms age), Weather is {wx_status}, Online ATC is {online_status}, Navdata is {nav_status}."
        else:
            conn = snapshot.get("connection_state", "DISCONNECTED")
            answer = f"System status: {conn}."
        return {
            "question": question,
            "answer": answer,
            "evidence": freshness
        }
        
    else:
        return {
            "question": question,
            "answer": f"OpenAIRAC flightdeck snapshot received for {snapshot.get('session_id')}.",
            "evidence": snapshot
        }


def main():
    print("================================================================================")
    print("OpenAIRAC 3.2 — AI Crew Gateway Integration Demo")
    print("================================================================================")
    
    # 1. Fetch flightdeck snapshot
    print(f"\n[1] Querying Flightdeck Snapshot from {DEFAULT_BASE_URL}/flightdeck/snapshot...")
    res = query_api("/flightdeck/snapshot")
    
    if "error" in res:
        print(f"[-] Note: Local simulator/map server not connected or inactive: {res['detail']}")
        print("[+] Demonstrating deterministic AI query logic with canonical fixture snapshot:")
        # Canonical fixture
        res = {
            "schema_version": "flightdeck_snapshot_v2",
            "session_id": "exec_UUEE_URFF_1724248800",
            "connection_state": "CONNECTED",
            "flight_phase": "CRUISE",
            "phase_evidence": "Enroute cruise (Alt FL360, GS 460 kt)",
            "aircraft": {"icao_type": "TU154", "cruise_altitude_ft": 36000},
            "origin": {"ident": "UUEE", "name": "Sheremetyevo", "selected_runway": "24C", "procedure_name": "EMGAS 3H"},
            "destination": {"ident": "URFF", "name": "Simferopol", "selected_runway": "19R", "procedure_name": "BURUD 2Y", "is_source_required": False},
            "position": {"latitude_deg": 52.41, "longitude_deg": 37.89, "altitude_msl_ft": 36000.0, "groundspeed_kts": 460.0, "vertical_speed_fpm": 0.0, "track_true_deg": 195.0, "on_ground": False},
            "active_leg": {"leg_index": 3, "leg_name": "EMGAS -> BURUD", "prev_fix": "EMGAS", "next_fix": "BURUD", "leg_type": "ATS_ROUTE", "desired_track_deg": 195.0},
            "navigation_geometry": {"xtk_nm": 0.2, "xtk_side": "RIGHT", "is_off_route": False, "distance_to_next_fix_nm": 84.2, "remaining_route_distance_nm": 385.4, "ete_next_fix_sec": 659, "ete_destination_sec": 3016},
            "descent_profile": {"tod_distance_nm": 42.5, "profile_status": "CRUISE_LEVEL", "required_descent_rate_fpm": -1850.0, "profile_deviation_ft": 0.0},
            "weather_summary": {"destination_metar": "URFF 19012KT 9999 SCT030 22/14 Q1013", "destination_runway_wind": {"runway_ident": "19R", "headwind_kts": 12.0, "crosswind_kts": 0.0, "is_tailwind": False, "is_recommended": True}},
            "advisories": [],
            "freshness": {
                "telemetry": "CURRENT",
                "weather": "CURRENT",
                "online": "CURRENT",
                "navdata": "CURRENT",
                "telemetry_age_ms": 150,
                "weather_age_sec": 120
            },
            "stale_flags": {"telemetry_stale": False, "telemetry_age_ms": 150}
        }
    
    # 2. Ask natural language crew questions
    questions = [
        "Where are we?",
        "What are we flying now?",
        "When is TOD?",
        "What's the weather at destination?",
        "What STAR are we flying?",
        "Is our data current?"
    ]
    
    for q in questions:
        result = ask_ai_crew(q, res)
        print(f"\nQ: {q}")
        print(f"A: {result['answer']}")
        if "route_tracking" in result:
            print(f"   [{result['route_tracking']}]")
            
    print("\n================================================================================")
    print("Deterministic AI Crew verification completed successfully.")
    print("================================================================================")


if __name__ == "__main__":
    main()
