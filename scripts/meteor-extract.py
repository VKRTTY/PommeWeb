#!/usr/bin/env python3
"""Reads Meteor Client's Java source and writes the module catalog Pomme's
Meteor port loads: every module's name, description and category, and each
setting's group, type, default, range and options.

    python3 scripts/meteor-extract.py <meteor-client checkout> <out.json>

Meteor Client is GPL-3.0 (github.com/MeteorDevelopment/meteor-client); the
catalog is derived from it and stays under the same license.
"""
import json
import re
import sys
from pathlib import Path

src = Path(sys.argv[1]) / "src/main/java/meteordevelopment/meteorclient"
out = Path(sys.argv[2])
modules_dir = src / "systems/modules"

def file_enums(text):
    """The enums a source file declares, by simple name."""
    found = {}
    for m in re.finditer(r"\benum\s+(\w+)\s*(?:implements[^{]*)?\{(.*?)(?:;|\})", text, re.S):
        body = re.sub(r"//[^\n]*|/\*.*?\*/", "", m.group(2), flags=re.S)
        values = []
        for part in re.split(r",(?![^(]*\))", body):
            name = re.match(r"\s*([A-Za-z_]\w*)", part)
            if name:
                values.append(name.group(1))
        if values:
            found.setdefault(m.group(1), values)
    return found


# Every enum in the source, by simple name, for settings whose enum lives in
# another file; a module's own enums come first.
enums = {}
for java in src.rglob("*.java"):
    for name, values in file_enums(java.read_text(errors="replace")).items():
        enums.setdefault(name, values)
# Minecraft enums some settings use.
enums.setdefault("LightLayer", ["SKY", "BLOCK"])
enums.setdefault("InteractionHand", ["MAIN_HAND", "OFF_HAND"])


def title(name):
    return " ".join(w[:1].upper() + w[1:] for w in name.split("-"))


def string_arg(chain, method):
    m = re.search(r"\.%s\(\s*\"((?:[^\"\\]|\\.)*)\"" % method, chain)
    return m.group(1).encode().decode("unicode_escape") if m else None


def raw_arg(chain, method):
    m = re.search(r"\.%s\(" % method, chain)
    if not m:
        return None
    depth, i = 1, m.end()
    while i < len(chain) and depth:
        depth += {"(": 1, ")": -1}.get(chain[i], 0)
        i += 1
    return chain[m.end():i - 1].strip()


def number(text):
    if text is None:
        return None
    text = text.strip().rstrip("dDfFL")
    try:
        return float(text) if "." in text or "e" in text.lower() else int(text)
    except ValueError:
        return None


def default(kind, enum_type, raw):
    if raw is None:
        return None
    if kind == "Bool":
        return raw == "true"
    if kind in ("Int", "Double", "Long"):
        return number(raw)
    if kind == "Enum":
        return raw.split(".")[-1]
    if kind == "Color":
        nums = [int(n) for n in re.findall(r"-?\d+", raw)]
        if len(nums) >= 3:
            return (nums + [255])[:4]
        return [255, 255, 255, 255]
    if kind == "Keybind":
        key = re.search(r"GLFW_KEY_(\w+)", raw)
        return key.group(1) if key else None
    if kind == "String":
        s = re.match(r'"((?:[^"\\]|\\.)*)"', raw)
        return s.group(1) if s else raw
    if kind.endswith("List"):
        return [x.lower() for x in re.findall(r"(?:Items|Blocks|EntityType|MobEffects|Enchantments)\.(\w+)", raw)]
    return raw


SETTING = re.compile(
    r"(?:public|private|protected)?\s*(?:final\s+)?Setting<[^=]*>\s+(\w+)\s*=\s*(\w+)\.add\(\s*new\s+(\w+?)Setting\.Builder(?:<([\w.]+)>)?\(\)(.*?)\.build\(\)\s*\)",
    re.S,
)
GROUP = re.compile(r"SettingGroup\s+(\w+)\s*=\s*settings\.(?:getDefaultGroup\(\)|createGroup\(\s*\"([^\"]+)\")")

# Registration order, which the GUI lists modules in.
order = []
modules_java = (modules_dir / "Modules.java").read_text()
for m in re.finditer(r"add\(new (\w+)\(\)\)", modules_java):
    order.append(m.group(1))

modules = []
for java in sorted(modules_dir.rglob("*.java")):
    text = java.read_text(errors="replace")
    head = re.search(r'super\(\s*Categories\.(\w+)\s*,\s*"([^"]+)"\s*,\s*"((?:[^"\\]|\\.)*)"', text)
    if not head:
        continue
    local = file_enums(text)
    groups = {}
    for g in GROUP.finditer(text):
        groups[g.group(1)] = g.group(2) or "General"
    settings = {}
    for s in SETTING.finditer(text):
        field, group_var, kind, enum_type, chain = s.groups()
        name = string_arg(chain, "name")
        if not name:
            continue
        setting = {
            "name": name,
            "title": title(name),
            "description": string_arg(chain, "description") or "",
            "type": kind,
            "default": default(kind, enum_type, raw_arg(chain, "defaultValue")),
        }
        if kind == "Enum":
            type_name = (enum_type or "").split(".")[-1]
            setting["options"] = local.get(type_name) or enums.get(type_name, [])
            if setting["default"] is None and setting["options"]:
                setting["default"] = setting["options"][0]
        rng = raw_arg(chain, "range")
        if rng:
            lo, hi = (number(x) for x in rng.split(",")[:2])
            setting["min"], setting["max"] = lo, hi
        for key in ("min", "max", "sliderMin", "sliderMax"):
            v = number(raw_arg(chain, key))
            if v is not None:
                setting[{"sliderMin": "slider_min", "sliderMax": "slider_max"}.get(key, key)] = v
        srange = raw_arg(chain, "sliderRange")
        if srange:
            lo, hi = (number(x) for x in srange.split(",")[:2])
            setting["slider_min"], setting["slider_max"] = lo, hi
        settings.setdefault(groups.get(group_var, "General"), []).append(setting)
    cls = java.stem
    modules.append({
        "class": cls,
        "name": head.group(2),
        "title": title(head.group(2)),
        "description": head.group(3).encode().decode("unicode_escape"),
        "category": head.group(1),
        "groups": [{"name": g, "settings": s} for g, s in settings.items()],
    })

rank = {c: i for i, c in enumerate(order)}
modules.sort(key=lambda m: (rank.get(m["class"], 10_000), m["name"]))
catalog = {
    "source": "MeteorDevelopment/meteor-client (GPL-3.0)",
    "categories": ["Combat", "Player", "Movement", "Render", "World", "Misc"],
    "modules": modules,
}
out.write_text(json.dumps(catalog, indent=1) + "\n")
counts = {}
for m in modules:
    counts[m["category"]] = counts.get(m["category"], 0) + 1
print(len(modules), "modules,", sum(len(s["settings"]) for m in modules for s in m["groups"]), "settings", counts)
