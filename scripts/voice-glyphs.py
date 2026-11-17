"""Builds the switched-off voice glyphs in src/ui/icons.rs from geometry: each glyph is sampled
as polylines, and every point closer to its mark than CLEAR is dropped, so the gap is even all
round. Run it from anywhere; it rewrites the five icon bodies in place:

    python scripts/voice-glyphs.py
"""
import math, pathlib, re

STEP = 0.05


def line(a, b):
    n = max(2, int(math.dist(a, b) / STEP))
    return [(a[0] + (b[0] - a[0]) * i / n, a[1] + (b[1] - a[1]) * i / n) for i in range(n + 1)]


def arc(c, r, a0, a1):
    """Angles in degrees, y down, sweeping from a0 to a1."""
    n = max(2, int(abs(math.radians(a1 - a0)) * r / STEP))
    return [(c[0] + r * math.cos(math.radians(a0 + (a1 - a0) * i / n)),
             c[1] + r * math.sin(math.radians(a0 + (a1 - a0) * i / n))) for i in range(n + 1)]


def chain(*parts):
    out = []
    for p in parts:
        out += p if not out else p[1:]
    return out


def mic(cx):
    # Lucide's mic: capsule 6 wide from y 2 to 15, a 7-radius cradle, a stand to y 22.
    capsule = chain(
        arc((cx, 5), 3, 180, 360),
        line((cx + 3, 5), (cx + 3, 12)),
        arc((cx, 12), 3, 0, 180),
        line((cx - 3, 12), (cx - 3, 5)),
    )
    cradle = chain(line((cx + 7, 10), (cx + 7, 12)), arc((cx, 12), 7, 0, 180), line((cx - 7, 12), (cx - 7, 10)))
    stand = line((cx, 19), (cx, 22))
    return [capsule, cradle, stand]


def headphones():
    # Lucide's headphones, drawn as two ear cups joined by the band, symmetric about x 12.
    left = chain(
        line((3, 12), (3, 19)), arc((5, 19), 2, 180, 90), line((5, 21), (6, 21)), arc((6, 19), 2, 90, 0),
        line((8, 19), (8, 16)), arc((6, 16), 2, 0, -90), line((6, 14), (3, 14)))
    right = chain(
        line((21, 12), (21, 19)), arc((19, 19), 2, 0, 90), line((19, 21), (18, 21)), arc((18, 19), 2, 90, 180),
        line((16, 19), (16, 16)), arc((18, 16), 2, 180, 270), line((18, 14), (21, 14)))
    band = arc((12, 12), 9, 180, 360)
    return [left, band, right]


def seg_dist(p, a, b):
    ax, ay = a; bx, by = b; px, py = p
    dx, dy = bx - ax, by - ay
    t = max(0, min(1, ((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)))
    return math.hypot(px - ax - t * dx, py - ay - t * dy)


def cut(paths, marks, clear):
    out = []
    for path in paths:
        run = []
        for p in path:
            if min(seg_dist(p, a, b) for a, b in marks) < clear:
                if len(run) > 1:
                    out.append(run)
                run = []
            else:
                run.append(p)
        if len(run) > 1:
            out.append(run)
    return out


def simplify(run, tol=0.012):
    """Douglas-Peucker: keeps the points needed to stay within `tol` of the sampled curve."""
    if len(run) < 3:
        return run
    a, b = run[0], run[-1]
    i, worst = max(((i, seg_dist(p, a, b)) for i, p in enumerate(run[1:-1], 1)), key=lambda x: x[1])
    if worst <= tol:
        return [a, b]
    return simplify(run[: i + 1], tol)[:-1] + simplify(run[i:], tol)


def svg_paths(runs, width="2"):
    attrs = f'fill="none" stroke="currentColor" stroke-width="{width}" stroke-linecap="round" stroke-linejoin="round"'
    out = []
    for run in runs:
        run = simplify(run)
        d = "M" + " L".join(f"{x:.2f} {y:.2f}" for x, y in run)
        out.append(f'<path d="{d}" {attrs}/>')
    return "".join(out)


def mark_svg(marks, width="2.6"):
    return "".join(
        f'<line x1="{a[0]:g}" x2="{b[0]:g}" y1="{a[1]:g}" y2="{b[1]:g}" fill="none" stroke="currentColor" stroke-width="{width}" stroke-linecap="round" stroke-linejoin="round"/>'
        for a, b in marks)


CLEAR = 3.6
STRIKE = [((3.5, 3.5), (20.5, 20.5))]
# The cross sits where the cradle meets the stand, on the right; the mic steps left to balance it.
MIC_X = 10.5
CX, CY, H = 18.75, 17.25, 2.75
CROSS = [((CX - H, CY - H), (CX + H, CY + H)), ((CX + H, CY - H), (CX - H, CY + H))]

def headphones_off():
    """Only the band is cut around the strike: the strike ends across the right cup, and a
    cup cut there falls apart into hooks, so both cups stay whole and it simply crosses."""
    left, band, right = headphones()
    return [left, right] + cut([band], STRIKE, CLEAR)


result = {
    "MIC_OFF_GLYPH": svg_paths(cut(mic(12), STRIKE, CLEAR)),
    "HEADPHONE_OFF_GLYPH": svg_paths(headphones_off()),
    "STRIKE": mark_svg(STRIKE),
    "MIC_DEACTIVATED_GLYPH": svg_paths(cut(mic(MIC_X), CROSS, CLEAR - 0.3)),
    "MIC_DEACTIVATED_CROSS": mark_svg(CROSS),
}
icons = pathlib.Path(__file__).resolve().parent.parent / "src" / "ui" / "icons.rs"
source = icons.read_text(encoding="utf-8")
for name, body in result.items():
    pattern = re.compile(
        r"(pub static " + name + r': Icon = Icon \{\s*name: "[^"]+",\s*view: 24\.0,\s*body: r#")(.*?)("#,)',
        re.S,
    )
    source, count = pattern.subn(lambda m: m.group(1) + body + m.group(3), source)
    assert count == 1, name
icons.write_bytes(source.encode("utf-8"))
