#!/usr/bin/env python3
"""Draws docs/progress.svg, a treemap of the port, from docs/progress.json."""
import json
import math
from pathlib import Path
from xml.sax.saxutils import escape

HERE = Path(__file__).parent
WIDTH = 1200
GROUP_HEIGHT = {"Graphics (WebGPU)": 330, "Platform": 250, "Game": 120}
BG, FG, DONE, TODO, LINE = "#0d1117", "#e6edf3", "#2ea043", "#6e7681", "#0d1117"


def squarify(items, x, y, w, h):
    """Bruls' squarified treemap: (item, x, y, w, h) per item, by weight."""
    total = sum(weight for _, weight in items)
    if not items or total <= 0:
        return []
    scale = w * h / total
    rest = [(item, weight * scale) for item, weight in items]
    out = []
    while rest:
        side = min(w, h)
        row, worst = [], math.inf
        for i in range(len(rest)):
            trial = row + [rest[i]]
            s = sum(a for _, a in trial)
            ratio = max(max(side * side * a / (s * s), s * s / (side * side * a)) for _, a in trial)
            if ratio > worst:
                break
            row, worst = trial, ratio
        rest = rest[len(row):]
        s = sum(a for _, a in row)
        if w >= h:
            col_w = s / h
            cy = y
            for item, a in row:
                out.append((item, x, cy, col_w, a / col_w))
                cy += a / col_w
            x, w = x + col_w, w - col_w
        else:
            row_h = s / w
            cx = x
            for item, a in row:
                out.append((item, cx, y, a / row_h, row_h))
                cx += a / row_h
            y, h = y + row_h, h - row_h
    return out


def item_svg(item, x, y, w, h):
    parts = [f'<rect x="{x:.1f}" y="{y:.1f}" width="{w:.1f}" height="{h:.1f}" fill="{TODO}"/>']
    total, done = item["total"], item["done"]
    cols = max(1, round(math.sqrt(total * w / h)))
    rows = math.ceil(total / cols)
    cw, ch = w / cols, h / rows
    for i in range(done):
        cx, cy = x + (i % cols) * cw, y + (i // cols) * ch
        parts.append(f'<rect x="{cx:.1f}" y="{cy:.1f}" width="{cw:.1f}" height="{ch:.1f}" fill="{DONE}"/>')
    if total > 1:
        for c in range(1, cols):
            parts.append(f'<line x1="{x + c * cw:.1f}" y1="{y:.1f}" x2="{x + c * cw:.1f}" y2="{y + h:.1f}" stroke="{LINE}" stroke-opacity="0.35"/>')
        for r in range(1, rows):
            parts.append(f'<line x1="{x:.1f}" y1="{y + r * ch:.1f}" x2="{x + w:.1f}" y2="{y + r * ch:.1f}" stroke="{LINE}" stroke-opacity="0.35"/>')
    parts.append(f'<rect x="{x:.1f}" y="{y:.1f}" width="{w:.1f}" height="{h:.1f}" fill="none" stroke="{BG}" stroke-width="3"/>')
    if w > 40 and h > 18:
        parts.append(f'<text x="{x + 6:.1f}" y="{y + 17:.1f}" class="item">{escape(item["name"])}</text>')
    return parts


def pct(done, total):
    return f"{100 * done / total:.2f}% ({done}/{total})"


def main():
    data = json.loads((HERE / "progress.json").read_text())
    groups = data["groups"]
    done = sum(i["done"] for g in groups for i in g["items"])
    total = sum(i["total"] for g in groups for i in g["items"])
    body, y = [], 70
    body.append(f'<text x="24" y="40" class="title">{escape(data["title"])}: {pct(done, total)}</text>')
    for group in groups:
        g_done = sum(i["done"] for i in group["items"])
        g_total = sum(i["total"] for i in group["items"])
        body.append(f'<text x="24" y="{y + 22}" class="group">{escape(group["name"])}: {pct(g_done, g_total)}</text>')
        top, height = y + 34, GROUP_HEIGHT.get(group["name"], 200)
        items = sorted(group["items"], key=lambda i: -i["total"])
        for item, ix, iy, iw, ih in squarify([(i, i["total"]) for i in items], 24, top, WIDTH - 48, height):
            body.extend(item_svg(item, ix, iy, iw, ih))
        y = top + height + 18
    body.append(f'<text x="24" y="{y + 16}" class="note">{escape(data["note"])}</text>')
    height = y + 34
    svg = f'''<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{height}" viewBox="0 0 {WIDTH} {height}">
<style>
text {{ font-family: -apple-system, "Segoe UI", Helvetica, Arial, sans-serif; fill: {FG}; }}
.title {{ font-size: 24px; font-weight: 600; }}
.group {{ font-size: 19px; }}
.item {{ font-size: 13px; paint-order: stroke; stroke: #000; stroke-width: 2.5px; stroke-opacity: 0.55; }}
.note {{ font-size: 13px; fill: #8b949e; }}
</style>
<rect width="100%" height="100%" fill="{BG}"/>
{chr(10).join(body)}
</svg>
'''
    (HERE / "progress.svg").write_text(svg)
    print(f"progress.svg: {pct(done, total)}")


if __name__ == "__main__":
    main()
