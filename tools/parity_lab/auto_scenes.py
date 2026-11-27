"""Discover tagged controls and replay read-only parity scenes."""

import argparse
from collections import Counter
import json
from pathlib import Path
import re
import subprocess
import sys
import time

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from shot_regress import PARITY, SIZE, capture, read, restore, write
import rust_probe as rp
from spec_delta import BASE_LINK, _exec_rust_action

DEFINITION = HERE / "spec_scenes_auto.json"
MODAL = "user.user-settings-modal.modal-root"
SAFE_POPOUTS = {"threads", "pinned-messages", "inbox", "composer-plus-menu", "emoji-picker", "user-area-popout"}


def screen_definitions():
    markdown = "/" + BASE_LINK.split("://", 1)[1]
    definitions = read(HERE / "spec_scenes_more.json")
    extra = read(HERE / "spec_scenes_extra.json")
    home = next(s for s in definitions if s["slug"] == "dm-home")
    friends = next(s for s in extra if s["slug"] == "friends-list")
    dm = next(s for s in definitions if s["slug"] == "dm-conversation")["rust"][0]["link"]
    voice = next(s for s in extra if s["slug"] == "voice-channel")["official"][0]["navigate"]
    rooms = {
        "guild-channel": {"route": markdown},
        "dm-home": {"route": markdown, "dm": "home", "source": home},
        "dm-conversation": {"route": "/" + dm.split("://", 1)[1]},
        "friends": {"route": markdown, "dm": "friends", "source": friends},
        "voice-preview": {"route": voice},
    }
    for s in definitions:
        if s["slug"].startswith("settings-"):
            rooms[s["slug"]] = dict(route=markdown, tab=s["official"][1]["click_text"], source=s)
    return rooms


def make_scenes(report, rooms):
    safe = {}
    for s in read(HERE / "spec_scenes.json"):
        if s["slug"] in SAFE_POPOUTS and len(s["official"]) == 1:
            match = re.fullmatch(r'\[data-flx="([^"]+)"\]', s["official"][0].get("click", ""))
            if match:
                safe[match[1]] = s
    for found in report:
        room_name = found["slug"]
        room = rooms[room_name]
        source = room.get("source")
        setup = [{"navigate": room["route"]}, *(source["official"] if source else [])]
        rust = source["rust"] if source else [{"link": "fluxer://" + room["route"].lstrip("/")}]
        for position, control in enumerate(found["controls"], 1):
            flx, index = control["flx"], control["idx"]
            selector = f"[data-flx={json.dumps(flx)}]"
            open_scene = safe.get(flx) if room_name == "guild-channel" and index == 0 else None
            for action in ("hover", "click") if open_scene else ("hover",):
                target = {"flx": flx, "index": index, "action": action}
                scene = dict(slug=f"auto-{room_name}-{position:03}-{action}", auto=True, room=room_name, target=target,
                             official=[*setup, {action: selector, "index": index}],
                             official_restore=[{"key": "Escape"}, {"navigate": "__BASE__"}],
                             rust=[*rust, {"wait_ms": 450}, {action: flx, "index": index}],
                             rust_expect_flx=flx, scope=[flx, "ui.tooltip."], wait_ms=600)
                if action == "click":
                    scene["expect_flx"] = open_scene["expect_flx"]
                    scene["rust_expect_flx"] = open_scene.get("rust_expect_flx", open_scene["expect_flx"])
                    scene["scope"] = [flx, *open_scene.get("scope", [open_scene["expect_flx"]])]
                elif room.get("tab"):
                    scene["expect_flx"] = MODAL
                yield scene


def run():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--out", type=Path, default=PARITY / "auto")
    p.add_argument("--rust-dir", type=Path, default=PARITY / "visual/latest")
    p.add_argument("--only", help="comma-separated room names to capture")
    p.add_argument("--capture", action="store_true")
    p.add_argument("--selftest", action="store_true")
    args = p.parse_args()
    rooms = screen_definitions()
    if args.selftest:
        record = [{"slug": "settings-my-account", "controls": [{"flx": "unknown", "idx": 2}]}]
        generated = list(make_scenes(record, rooms))
        assert len(generated) == 1 and generated[0]["target"]["action"] == "hover"
        assert generated[0]["rust"][-1] == {"hover": "unknown", "index": 2}
        assert generated[0]["official_restore"][-1] == {"navigate": "__BASE__"}
        print("auto_scenes selftest PASS")
        return 0
    selected = args.only.split(",") if args.only else list(rooms)
    if missing := set(selected) - rooms.keys():
        p.error(f"unknown rooms: {', '.join(sorted(missing))}")
    args.out.mkdir(parents=True, exist_ok=True)
    requested = args.out / "rooms.json"
    write(requested, [dict(slug=slug, **{k: v for k, v in room.items() if k != "source"})
                      for slug, room in rooms.items()])
    code = subprocess.run(["node", str(HERE / "crawl.mjs"), "--attach", "9334", "--origin", "fluxer-app://app",
                           "--route", rooms["guild-channel"]["route"], "--out", str(args.out), "--inventory", str(requested)],
                          timeout=300).returncode
    if code > 1:
        return code
    inventory = read(args.out / "inventory.json")
    all_scenes = list(make_scenes(inventory, rooms))
    DEFINITION.write_text(
        "[\n" + ",\n".join("  " + json.dumps(scene, ensure_ascii=False) for scene in all_scenes) + "\n]\n",
        encoding="utf-8",
    )
    clicked = {(s["room"], s["target"]["flx"], s["target"]["index"]) for s in all_scenes
               if s["target"]["action"] == "click"}
    write(args.out / "unclassified_clicks.json", [
        {"room": s["room"], **s["target"]} for s in all_scenes
        if s["target"]["action"] == "hover" and (s["room"], s["target"]["flx"], s["target"]["index"]) not in clicked
    ])
    if not args.capture:
        print(f"auto scenes={len(all_scenes)} rooms={len(inventory)} room_failures={sum(r['status'] != 'ok' for r in inventory)}")
        return code
    args.rust_dir.mkdir(parents=True, exist_ok=True)
    rows = []
    try:
        rp.send("probe_resize", w=SIZE[0], h=SIZE[1])
        restore()
        for name in selected:
            ready = []
            for scene in (s for s in all_scenes if s["room"] == name):
                target = scene["target"]
                row = {"slug": scene["slug"], "room": name, "target": target, "status": "matched"}
                rows.append(row)
                try:
                    if name == "voice-preview":
                        row["status"] = "parked"
                        row["error"] = "Rust voice entry may join the channel"
                        continue
                    rp.send("probe_key", keystroke="escape")
                    for act in scene["rust"][:-2]:
                        _exec_rust_action(rp, act, time)
                        if "link" in act:
                            time.sleep(0.3)
                    frame = rp.inspect_until(
                        lambda f: sum(e["flx"] == target["flx"] for e in f["elements"]) > target["index"], 1.2)
                    if sum(e["flx"] == target["flx"] for e in frame["elements"]) <= target["index"]:
                        row["status"] = "unmatched"
                        continue
                    capture({**scene, "rust": scene["rust"][-2:]}, args.rust_dir)
                    ready.append(scene["slug"])
                except (OSError, RuntimeError, ValueError) as error:
                    row.update(status="rust_capture_error", error=str(error))
                finally:
                    rp.send("probe_key", keystroke="escape")
                    write(args.out / "auto_report.json", rows)
            if not ready:
                continue
            official_dir = PARITY / "official/1124/auto"
            cmd = ["node", str(HERE / "spec_official.mjs"), "--attach", "9334", "--viewport", "1280x800",
                   "--expected-web-build", "Web 2026.1124", "--scenes", str(DEFINITION), "--scene-bundles",
                   "--skip-captured", "--out-dir", str(official_dir), "--only", ",".join(ready)]
            result = subprocess.run(cmd, timeout=480).returncode
            official = {r["slug"]: r for r in read(official_dir / "scenes.report.json")["scenes"]}
            for row in rows:
                if row["slug"] in ready and official.get(row["slug"], {}).get("status") not in {"ok", "skipped"}:
                    row.update(status="official_capture_error", error=official.get(row["slug"], {}).get("error"))
            write(args.out / "auto_report.json", rows)
            if result > 1:
                return result
    finally:
        restore()
    counts = Counter(r["status"] for r in rows)
    print(f"auto scenes={len(all_scenes)} {dict(counts)}")
    return int(code != 0 or any(k.endswith("_error") for k in counts))


if __name__ == "__main__":
    raise SystemExit(run())
