"""Builds the three chat-layout mockups as standalone HTML (one per style) plus an overview.

    python3 docs/chat-proposals/build.py

Colours, sizes and type follow docs/design-system.md (Flexspeak dark palette).
"""

import re
from pathlib import Path

HERE = Path(__file__).parent

CSS = """
:root{
  --canvas:#1c1d20;--chrome:#232427;--surface:#26272b;--stage:#151618;--raised:#34353a;
  --hair:rgba(255,255,255,.08);--wash:rgba(255,255,255,.05);--recess:rgba(0,0,0,.22);
  --text:#ebebee;--muted:#a0a1a8;--faint:#75767e;--accent:#8ea8ff;--accent-tint:rgba(142,168,255,.16);
  --good:#6fdc9a;--fair:#f2c46b;--poor:#ff7a8a;--live:#ff5a52;
  --dm:#c9a6ff;--dm-tint:rgba(201,166,255,.12);--server:#f2c46b;
}
*{box-sizing:border-box;margin:0;padding:0}
body{background:#0e0f11;font-family:Inter,-apple-system,"Segoe UI",system-ui,sans-serif;font-size:13px;color:var(--text);
  -webkit-font-smoothing:antialiased}
.win{width:1120px;height:700px;background:var(--canvas);display:flex;flex-direction:column;overflow:hidden;border-radius:10px;
  border:1px solid var(--hair)}
.hdr{height:52px;display:flex;align-items:center;padding:0 12px 0 84px;gap:8px;position:relative;flex:none}
.hdr .lights{position:absolute;left:18px;top:20px;display:flex;gap:8px}
.hdr .lights i{width:12px;height:12px;border-radius:50%;background:#ff5f57}
.hdr .lights i:nth-child(2){background:#febc2e}.hdr .lights i:nth-child(3){background:#28c840}
.hdr .title{color:var(--muted);font-weight:600;flex:1}
.ib{width:28px;height:28px;border-radius:6px;display:grid;place-items:center;color:var(--muted);flex:none}
.ib.on{background:var(--raised);color:var(--text)}
svg{width:16px;height:16px;stroke:currentColor;fill:none;stroke-width:1.75;stroke-linecap:round;stroke-linejoin:round;flex:none}
.sm svg,svg.sm{width:13px;height:13px}
.body{flex:1;display:flex;min-height:0;border-top:1px solid var(--hair)}
.side{width:260px;background:var(--surface);padding:12px;display:flex;flex-direction:column;gap:8px;flex:none;border-right:1px solid var(--hair)}
.srv{display:flex;align-items:center;gap:8px}
.tile{width:28px;height:28px;border-radius:6px;background:#b8478f;color:#fff;font-size:11.5px;font-weight:650;display:grid;place-items:center}
.srv b{font-size:14px;font-weight:620;flex:1}
.tree{flex:1;display:flex;flex-direction:column;gap:2px;overflow:hidden}
.ch,.mem{height:26px;border-radius:6px;display:flex;align-items:center;gap:6px;padding:0 6px}
.ch .chev{color:var(--faint)}
.ch.cur{color:var(--accent);font-weight:600;background:var(--accent-tint)}
.ch.people{color:var(--text)} .ch.empty{color:var(--muted)} .ch.locked{color:var(--faint)}
.mem{padding-left:38px;color:var(--text)}
.mem.sel{background:var(--accent-tint)}
.ring{width:14px;height:14px;border-radius:50%;border:1.5px solid var(--faint);flex:none}
.ring.talk{border-color:var(--good);background:var(--good)}
.name{flex:1;min-width:0;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.d1{padding-left:20px}.mem.d1{padding-left:52px}
.badge{min-width:18px;height:16px;border-radius:8px;padding:0 5px;font-size:10.5px;font-weight:650;display:grid;place-items:center;
  background:var(--accent);color:#14161c}
.badge.dm{background:var(--dm)} .badge.quiet{background:var(--raised);color:var(--muted)}
.log{border-top:1px solid var(--hair);padding-top:8px;display:flex;flex-direction:column;gap:2px}
.ovl{font-size:11px;font-weight:600;color:var(--faint);display:flex;align-items:center;gap:6px;height:20px}
.logrow{height:22px;display:flex;align-items:center;gap:6px;color:var(--muted);font-size:11.5px}
.logrow .t{color:var(--faint);font-variant-numeric:tabular-nums}
.voice{display:flex;flex-direction:column;gap:8px;padding-top:4px}
.dock{display:flex;gap:6px}
.dock .ib{flex:1;height:32px;background:var(--raised);color:var(--text)}
.dock .ib.hang{background:#d9434f;color:#fff}
.meter{height:4px;border-radius:2px;background:var(--recess);overflow:hidden}
.meter i{display:block;height:100%;width:34%;background:var(--good)}
.stage{flex:1;background:var(--stage);display:flex;flex-direction:column;min-width:0}
.stage-h{padding:16px 20px 0;display:flex;align-items:baseline;gap:8px}
.stage-h h1{font-size:17px;font-weight:600}
.stage-h span{color:var(--muted)}
.empty-line{flex:1;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:12px;color:var(--muted)}
.btns{display:flex;gap:8px}
.btn{height:28px;padding:0 12px;border-radius:6px;display:inline-flex;align-items:center;gap:6px;font-weight:500;background:var(--raised);color:var(--text);white-space:nowrap}
.btn.pri{background:var(--accent);color:#14161c}
.btn.ghost{background:transparent;color:var(--muted)}
.foot{height:40px;display:flex;align-items:center;gap:18px;padding:0 14px;color:var(--muted);font-size:11.5px;border-top:1px solid var(--hair);flex:none}
.foot .dot{width:7px;height:7px;border-radius:50%;background:var(--good);display:inline-block;margin-right:6px}
.foot .g{color:var(--good)}
.spark{width:56px;height:16px;display:inline-block;vertical-align:middle;margin-left:6px}
.foot .grow{flex:1}

/* chat */
.chat{display:flex;flex-direction:column;min-height:0;background:var(--surface)}
.msgs{flex:1;overflow:hidden;padding:10px 14px;display:flex;flex-direction:column;gap:10px;justify-content:flex-end}
.m{display:flex;gap:9px}
.av{width:26px;height:26px;border-radius:50%;flex:none;display:grid;place-items:center;font-size:11px;font-weight:650;color:#fff}
.m .who{font-weight:600}
.m .when{color:var(--faint);font-size:11px;margin-left:6px;font-variant-numeric:tabular-nums}
.m p{color:var(--text);line-height:1.4;margin-top:1px}
.m .meta{display:flex;align-items:center;gap:6px}
.sysline{color:var(--faint);font-size:11.5px;text-align:center}
.compose{margin:0 12px 12px;min-height:36px;border-radius:8px;background:var(--recess);border:1px solid var(--hair);display:flex;align-items:center;gap:8px;padding:0 6px 0 10px;color:var(--faint)}
.compose .ph{flex:1}
.compose .send{color:var(--accent)}
.chip{height:20px;border-radius:10px;padding:0 8px;display:inline-flex;align-items:center;gap:4px;font-size:11px;font-weight:600;white-space:nowrap}
.chip.ch{background:var(--accent-tint);color:var(--accent)}
.chip.dm{background:var(--dm-tint);color:var(--dm)}
.chip.srv{background:rgba(242,196,107,.13);color:var(--server)}
.chip.plain{background:var(--raised);color:var(--muted)}
.chip.sel{background:var(--text);color:#16171a}
.c-mara{background:#c0607a}.c-theo{background:#4f9e7c}.c-ines{background:#8a6bd6}.c-jan{background:#5b7be0}.c-lu{background:#c48a3c}
.typing{color:var(--faint);font-size:11px;padding:0 14px 6px}

/* annotation callouts */
.pin{position:absolute;width:20px;height:20px;border-radius:50%;background:#ffd84d;color:#2a2300;font-size:11px;font-weight:750;
  display:grid;place-items:center;box-shadow:0 0 0 3px rgba(0,0,0,.55)}
.legend{display:grid;grid-template-columns:repeat(3,1fr);gap:16px;padding:16px 4px 0;color:#c9cad0;font-size:12.5px;line-height:1.45}
.legend div{display:flex;gap:9px}
.legend b{flex:none;width:20px;height:20px;border-radius:50%;background:#ffd84d;color:#2a2300;font-size:11px;display:grid;place-items:center}
.ttl{font-size:16px;font-weight:650;color:#fff;padding:0 4px 12px}.ttl span{color:#9a9ba2;font-weight:500;margin-left:8px}
.frame{position:relative;width:1120px}
.stream{flex:1;background:#000;display:flex;align-items:center;justify-content:center;min-width:0;position:relative}
.stream .pic{width:100%;aspect-ratio:16/9;background:repeating-linear-gradient(135deg,#1d3a66 0 34px,#25508c 34px 68px)}
.edge{position:absolute;right:0;top:52px;bottom:40px;width:12px}
.edge i{position:absolute;right:2px;top:50%;width:3px;height:44px;margin-top:-22px;border-radius:2px;background:var(--faint)}
.edge.near i{width:5px;height:60px;margin-top:-30px;background:var(--muted)}
.edge .ndot{position:absolute;right:0;top:50%;margin-top:-36px;width:10px;height:10px;border-radius:50%;box-shadow:0 0 0 2px #000}
.toast{position:absolute;right:16px;top:64px;width:300px;background:var(--raised);border:1px solid var(--hair);border-radius:10px;padding:10px 12px;
  box-shadow:0 8px 24px rgba(0,0,0,.45);display:flex;gap:10px}
.toast.dm{border-color:rgba(201,166,255,.35)}
.toast p{color:var(--text);line-height:1.4;margin-top:2px}
.toast .acts{display:flex;gap:6px;margin-top:8px}
.toast .btn{height:24px;font-size:12px;padding:0 10px}
.peek{position:absolute;right:8px;top:60px;bottom:48px;width:340px;border-radius:12px;overflow:hidden;border:1px solid var(--hair);
  box-shadow:0 8px 24px rgba(0,0,0,.4);display:flex}
.peek .chat{background:var(--raised);width:100%}
.peek .compose{background:rgba(0,0,0,.25)}

"""

I = {
    "hash": '<svg><path d="M4 9h16M4 15h16M10 3 8 21M16 3l-2 18"/></svg>',
    "globe": '<svg><circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18"/></svg>',
    "user": '<svg><circle cx="12" cy="8" r="4"/><path d="M4 21a8 8 0 0 1 16 0"/></svg>',
    "lock": '<svg class="sm"><rect x="5" y="11" width="14" height="10" rx="2"/><path d="M8 11V7a4 4 0 0 1 8 0v4"/></svg>',
    "spk": '<svg class="sm"><path d="M11 5 6 9H3v6h3l5 4z"/><path d="M15.5 8.5a5 5 0 0 1 0 7M18.5 5.5a9 9 0 0 1 0 13"/></svg>',
    "chev": '<svg class="sm chev"><path d="m6 9 6 6 6-6"/></svg>',
    "chevr": '<svg class="sm chev"><path d="m9 6 6 6-6 6"/></svg>',
    "mic": '<svg><rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5 11a7 7 0 0 0 14 0M12 18v3"/></svg>',
    "head": '<svg><path d="M3 18v-6a9 9 0 0 1 18 0v6"/><path d="M21 19a2 2 0 0 1-2 2h-1v-6h3zM3 19a2 2 0 0 0 2 2h1v-6H3z"/></svg>',
    "screen": '<svg><rect x="2" y="3" width="20" height="14" rx="2"/><path d="M8 21h8M12 17v4"/></svg>',
    "hang": '<svg><path d="M3 15c4.5-4 13.5-4 18 0l-2 3-4-1.5v-2.5a10 10 0 0 0-6 0v2.5L5 18z"/></svg>',
    "send": '<svg><path d="m22 2-7 20-4-9-9-4z"/><path d="M22 2 11 13"/></svg>',
    "x": '<svg class="sm"><path d="M18 6 6 18M6 6l12 12"/></svg>',
    "eye": '<svg><path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12z"/><circle cx="12" cy="12" r="3"/></svg>',
    "chart": '<svg><path d="M3 3v18h18M7 15l4-4 3 3 5-6"/></svg>',
    "gear": '<svg><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z"/></svg>',
    "chat": '<svg><path d="M21 12a8 8 0 0 1-11.6 7.1L3 21l1.9-6.4A8 8 0 1 1 21 12z"/></svg>',
    "reply": '<svg class="sm"><path d="M9 14 4 9l5-5"/><path d="M20 20v-7a4 4 0 0 0-4-4H4"/></svg>',
    "arrowl": '<svg class="sm"><path d="M19 12H5M12 19l-7-7 7-7"/></svg>',
    "plus": '<svg class="sm"><path d="M12 5v14M5 12h14"/></svg>',
    "pclose": '<svg><rect x="3" y="3" width="18" height="18" rx="2"/><path d="M15 3v18M8 10l2 2-2 2"/></svg>',
    "server": '<svg><rect x="3" y="4" width="18" height="7" rx="2"/><rect x="3" y="13" width="18" height="7" rx="2"/><path d="M7 7.5h.01M7 16.5h.01"/></svg>',
    "palette": '<svg><circle cx="12" cy="12" r="9"/><circle cx="8" cy="10" r="1"/><circle cx="12" cy="7.5" r="1"/><circle cx="16" cy="10" r="1"/><path d="M12 21a3 3 0 0 1 0-6h2a3 3 0 0 0 3-3"/></svg>',
    "wave": '<svg><path d="M3 10v4M7 7v10M11 4v16M15 8v8M19 10v4"/></svg>',
    "kbd": '<svg><rect x="2" y="6" width="20" height="12" rx="2"/><path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M7 14h10"/></svg>',
    "shield": '<svg><path d="M12 3 4 6v6c0 5 3.5 8 8 9 4.5-1 8-4 8-9V6z"/></svg>',
    "download": '<svg><path d="M12 3v12M7 10l5 5 5-5M4 21h16"/></svg>',
    "alert": '<svg><path d="M12 3 2 20h20zM12 10v4M12 17h.01"/></svg>',
    "file": '<svg><path d="M14 3H6v18h12V7z"/><path d="M14 3v4h4M9 13h6M9 17h6"/></svg>',
    "search": '<svg><circle cx="11" cy="11" r="7"/><path d="m20 20-4-4"/></svg>',
    "share": '<svg><rect x="2" y="3" width="20" height="14" rx="2"/><path d="M8 21h8M12 17v4M9 10l3-3 3 3M12 7v6"/></svg>',
    "check": '<svg class="sm"><path d="M20 6 9 17l-5-5"/></svg>',
    "filter": '<svg class="sm"><path d="M3 5h18l-7 8v6l-4 2v-8z"/></svg>',
}

I = {k: re.sub(r"<svg(?![^>]*viewBox)", '<svg viewBox="0 0 24 24"', v) for k, v in I.items()}

SPARK = '<svg class="spark" viewBox="0 0 56 16" style="stroke:var(--good);stroke-width:1.4"><path d="M0 12 8 11 16 12 24 9 32 11 40 10 48 12 56 10"/></svg>'


def header():
    return f"""<div class="hdr"><div class="lights"><i></i><i></i><i></i></div>
<div class="title">Flexspeak</div>
<div class="ib">{I['chart']}</div><div class="ib">{I['gear']}</div></div>"""


def footer():
    return f"""<div class="foot"><span class="g"><span class="dot"></span>Good</span>
<span>Loss in 0.0%{SPARK}</span><span>Loss out 0.1%{SPARK}</span><span>Ping 24 ms{SPARK}</span>
<span class="grow"></span><span>CPU 2.1%</span><span>RAM 84 MB</span></div>"""


def member(name, cls="", talk=False, extra="", depth=""):
    return f'<div class="mem {depth} {cls}"><span class="ring{" talk" if talk else ""}"></span><span class="name">{name}</span>{extra}</div>'


def tree(lobby_extra="", raid_extra="", mem_extra=None, sel=None):
    mem_extra = mem_extra or {}
    sel = sel or set()
    return f"""<div class="tree">
<div class="ch cur">{I['chev']}{I['spk']}<span class="name">Lobby</span>{lobby_extra}</div>
{member("Jan (you)", talk=True)}
{member("Mara", "sel" if "Mara" in sel else "", extra=mem_extra.get("Mara", ""))}
{member("Theo", "sel" if "Theo" in sel else "", extra=mem_extra.get("Theo", ""))}
<div class="ch people d1">{I['chev']}{I['spk']}<span class="name">Raid night</span>{raid_extra}</div>
{member("Ines", "sel" if "Ines" in sel else "", depth="d1", extra=mem_extra.get("Ines", ""))}
<div class="ch locked d1"><span style="width:13px"></span>{I['lock']}<span class="name">Focus room</span></div>
<div class="ch empty"><span style="width:13px"></span>{I['spk']}<span class="name">AFK</span></div>
</div>"""


def log():
    return f"""<div class="log"><div class="ovl">{I['chev']}Server log</div>
<div class="logrow"><span class="t">21:02</span>Ines moved to Raid night</div>
<div class="logrow"><span class="t">21:04</span>Theo joined Lobby</div></div>"""


def voice():
    return f"""<div class="voice"><div class="dock"><div class="ib">{I['mic']}</div><div class="ib">{I['head']}</div>
<div class="ib">{I['screen']}</div><div class="ib hang">{I['hang']}</div></div><div class="meter"><i></i></div></div>"""


def sidebar(tree_html, with_log=True, extra=""):
    return f"""<div class="side"><div class="srv"><div class="tile">ZI</div><b>Zirkumflex</b><div class="ib">{I['eye']}</div></div>
{tree_html}{extra}{log() if with_log else ""}{voice()}</div>"""


def stage(compact=False):
    if compact:
        return f"""<div class="stage-h"><h1>Lobby</h1><span>3 people</span></div>
<div class="empty-line" style="flex:none;padding:18px 0 22px"><div>No one is sharing a screen.</div>
<div class="btns"><span class="btn pri">{I['screen']}Share Main Screen</span><span class="btn">More Options</span></div></div>"""
    return f"""<div class="stage-h"><h1>Lobby</h1><span>3 people</span></div>
<div class="empty-line"><div>No one is sharing a screen.</div>
<div class="btns"><span class="btn pri">{I['screen']}Share Main Screen</span><span class="btn">More Options</span></div></div>"""


def msg(who, cls, when, text, chip="", style=""):
    return f"""<div class="m" style="{style}"><div class="av {cls}">{who[0]}</div><div style="min-width:0;flex:1">
<div class="meta"><span class="who">{who}</span>{chip}<span class="when">{when}</span></div><p>{text}</p></div></div>"""



# ---------- chat panels ----------

def tabs_bar(collapse=True):
    tab = lambda inner, cls="": f'<div style="height:32px;padding:0 7px;display:flex;align-items:center;gap:5px;white-space:nowrap;{cls}">{inner}</div>'
    return (f'<div style="display:flex;align-items:center;gap:0;padding:8px 6px 0 8px;border-bottom:1px solid var(--hair);height:41px;flex:none">'
            + tab(f"{I['hash']}Lobby", "color:var(--accent);font-weight:600;border-bottom:2px solid var(--accent)")
            + tab(f"{I['globe']}Server", "color:var(--muted)")
            + tab('<span class="av c-mara" style="width:16px;height:16px;font-size:9px">M</span>Mara<span class="badge dm">2</span>', "color:var(--text)")
            + tab('<span class="av c-ines" style="width:16px;height:16px;font-size:9px">I</span>Ines', "color:var(--muted)")
            + '<div style="flex:1"></div>'
            + (f'<div class="ib" title="Hide chat">{I["pclose"]}</div>' if collapse else "")
            + "</div>")


def chat_a(width=340, collapse=True, extra_style=""):
    msgs = "".join([
        msg("Theo", "c-theo", "21:04", "back, had to restart my router"),
        msg("Mara", "c-mara", "21:05", "we start the raid at half past, Ines is setting up"),
        msg("Jan", "c-jan", "21:06", "👍 I'll join after this match"),
        msg("Theo", "c-theo", "21:07", "can someone share the boss notes link again?"),
    ])
    w = f"width:{width}px;" if width else "flex:1;"
    return f"""<div class="chat" style="{w}border-left:1px solid var(--hair);flex:none;{extra_style}">{tabs_bar(collapse)}
<div style="padding:10px 14px 0;color:var(--faint);font-size:11.5px;display:flex;align-items:center;gap:6px">{I['hash']}<span>Everyone in <b style="color:var(--muted)">Lobby</b> sees this</span></div>
<div class="msgs">{msgs}</div><div class="typing">Mara is typing…</div>
<div class="compose"><span class="ph">Message #Lobby</span><span class="ib send">{I['send']}</span></div></div>"""


def sm(icon):
    return I[icon].replace('<svg viewBox', '<svg class="sm" viewBox', 1)


def chat_b(width=340, collapse=True, extra_style=""):
    filters = f"""<div style="display:flex;align-items:center;gap:6px;padding:0 6px 0 12px;border-bottom:1px solid var(--hair);height:41px;flex:none">
<span class="chip sel">All</span><span class="chip plain">{sm('hash')}Lobby</span><span class="chip plain">{sm('globe')}Server</span>
<span class="chip plain">{sm('user')}Private <span class="badge dm" style="height:14px;min-width:14px;font-size:9.5px">2</span></span>
<span style="flex:1"></span>{f'<div class="ib">{I["pclose"]}</div>' if collapse else ''}</div>"""
    dm_box = "background:var(--dm-tint);border-radius:8px;padding:7px 9px;margin:0 -4px"
    msgs = "".join([
        msg("Lukas", "c-lu", "20:58", "Server restarts at 4am for the update", f'<span class="chip srv">{sm("globe")}Server</span>'),
        msg("Theo", "c-theo", "21:04", "back, had to restart my router", '<span class="chip ch">#Lobby</span>'),
        msg("Mara", "c-mara", "21:06", "psst, can you bring the spare key?", f'<span class="chip dm">{sm("user")}to you</span>', dm_box),
        msg("Jan", "c-jan", "21:06", "yes, got it 👍", f'<span class="chip dm">{sm("user")}to Mara</span>', dm_box),
        msg("Theo", "c-theo", "21:07", "can someone share the boss notes link again?", '<span class="chip ch">#Lobby</span>'),
    ])
    w = f"width:{width}px;" if width else "flex:1;"
    return f"""<div class="chat" style="{w}border-left:1px solid var(--hair);flex:none;{extra_style}">{filters}
<div class="msgs">{msgs}</div>
<div class="compose" style="padding-left:6px"><span class="chip ch" style="height:24px;border-radius:6px">#Lobby {sm('chev').replace('chev','')}</span>
<span class="ph">Message everyone in Lobby</span><span class="ib send">{I['send']}</span></div></div>"""


def stream_stage(full=False):
    return f'<div class="stream"><div class="pic"></div></div>'


def tree_streaming(mem_extra=None):
    t = tree(mem_extra=mem_extra)
    live = '<svg viewBox="0 0 24 24" class="sm" style="color:var(--live)"><rect x="2" y="4" width="20" height="13" rx="2"/><path d="M8 21h8M12 17v4"/></svg>'
    return t.replace('<span class="name">Mara</span>', f'<span class="name">Mara</span>{live}', 1)


def window(body, overlay=""):
    return f"""<div class="win" style="position:relative">{header()}<div class="body">{body}</div>{footer()}{overlay}</div>"""


# ---------- screens ----------

def s_tabs_stream():
    win = window(sidebar(tree_streaming(mem_extra={"Mara": '<span class="badge dm">2</span>'})) + stream_stage() + chat_a())
    return win, [
        (746, 64, "1", "Mara's stream keeps the stage; the chat column sits beside it at 340 pt."),
        (1092, 100, "2", "Hide chat (panel icon) gives the stream the full width. ⌘⇧C toggles it too."),
        (12, 159, "3", "Unread private messages also show on the person in the tree."),
    ]


def s_feed_stream():
    win = window(sidebar(tree_streaming()) + stream_stage() + chat_b())
    return win, [
        (746, 64, "1", "Same column, one feed: filters narrow it, tags say where each line went."),
        (746, 430, "2", "Private lines stay tinted violet, so they stand out next to the stream."),
        (746, 614, "3", "The box shows its target as a chip; reply to a private line to switch it."),
    ]


def s_collapsed():
    win = window(sidebar(tree_streaming()) + stream_stage(), '<div class="edge"><i></i></div>')
    return win, [
        (1066, 346, "1", "Hidden: the stream runs to the window edge. A thin handle stays on the right, the same as the server rail's on the left."),
        (1066, 400, "2", "Hover the handle and it grows. Click it to bring chat back; hover alone peeks."),
        (520, 360, "3", "Nothing else covers the picture while no message comes in."),
    ]


def toast(who, cls, text, chip, dm=False):
    btns = (f'<div class="acts"><span class="btn pri">{sm("reply")}Reply</span><span class="btn ghost">Open Chat</span></div>')
    return f"""<div class="toast{' dm' if dm else ''}"><div class="av {cls}">{who[0]}</div><div style="flex:1;min-width:0">
<div class="meta" style="display:flex;align-items:center;gap:6px"><b>{who}</b>{chip}<span style="flex:1"></span><span style="color:var(--faint)">{sm('x')}</span></div>
<p>{text}</p>{btns}</div></div>"""


def s_collapsed_channel():
    t = toast("Theo", "c-theo", "can someone share the boss notes link again?", '<span class="chip ch">#Lobby</span>')
    edge = '<div class="edge"><i></i><span class="ndot" style="background:var(--accent)"></span></div>'
    win = window(sidebar(tree_streaming()) + stream_stage(), t + edge)
    return win, [
        (774, 70, "1", "A new message while chat is hidden pops up top right for 5 s, then leaves. Hover holds it; Reply answers right there."),
        (1066, 346, "2", "The handle gets a dot until you open chat: blue for the channel or server."),
        (520, 360, "3", "Shown here with Tabs; One feed uses the same pop-up and dot."),
    ]


def s_collapsed_private():
    t = toast("Mara", "c-mara", "psst, can you bring the spare key for the vault run?", f'<span class="chip dm">{sm("user")}to you</span>', dm=True)
    edge = '<div class="edge"><i></i><span class="ndot" style="background:var(--dm)"></span></div>'
    win = window(sidebar(tree_streaming(mem_extra={"Mara": '<span class="badge dm">1</span>'})) + stream_stage(), t + edge)
    return win, [
        (774, 70, "1", "A private message pops up with the violet edge and \"to you\", so it can't pass for channel talk."),
        (1066, 346, "2", "The dot turns violet for a private message and wins over blue."),
        (12, 159, "3", "The tree marks the sender too, so it's still findable after the pop-up leaves."),
    ]


def s_peek():
    peek = f'<div class="peek">{chat_a(width=0, collapse=False)}</div>'
    win = window(sidebar(tree_streaming()) + stream_stage(), peek + '<div class="edge near"></div>')
    return win, [
        (756, 66, "1", "Hovering the handle peeks the chat over the stream, like the server rail does on the left. Moving away hides it again."),
        (756, 614, "2", "Typing or clicking into it keeps it open until you move away and the box is empty."),
        (520, 360, "3", "Click the handle (or ⌘⇧C) to dock it back next to the stream."),
    ]


# ---------- settings ----------

def skeleton(kind):
    bar = lambda w, c="var(--raised)", h=6: f'<i style="display:block;height:{h}px;width:{w}%;border-radius:3px;background:{c}"></i>'
    if kind == "a":
        top = ('<div style="display:flex;gap:6px;align-items:flex-end;border-bottom:1px solid var(--hair);padding-bottom:5px">'
               '<i style="width:34px;height:6px;border-radius:3px;background:var(--accent)"></i>'
               '<i style="width:26px;height:6px;border-radius:3px;background:var(--raised)"></i>'
               '<i style="width:26px;height:6px;border-radius:3px;background:var(--raised)"></i>'
               '<i style="width:8px;height:8px;border-radius:50%;background:var(--dm)"></i></div>')
        rows = "".join(f'<div style="display:flex;gap:6px;align-items:center"><i style="width:10px;height:10px;border-radius:50%;background:var(--raised)"></i>{bar(w)}</div>' for w in (70, 55, 80))
    else:
        top = ('<div style="display:flex;gap:5px;border-bottom:1px solid var(--hair);padding-bottom:5px">'
               + "".join(f'<i style="width:{w}px;height:8px;border-radius:4px;background:{c}"></i>' for w, c in ((18, "var(--text)"), (24, "var(--raised)"), (24, "var(--raised)"), (28, "var(--raised)")))
               + "</div>")
        row = lambda c, w, tint="": f'<div style="display:flex;gap:6px;align-items:center;{tint}"><i style="width:10px;height:10px;border-radius:50%;background:var(--raised)"></i><i style="width:16px;height:6px;border-radius:3px;background:{c}"></i>{bar(w)}</div>'
        rows = (row("var(--accent)", 55) + row("var(--dm)", 60, "background:var(--dm-tint);border-radius:4px;padding:3px;margin:-3px")
                + row("var(--server)", 45))
    return f"""<div style="height:118px;border-radius:6px;background:var(--canvas);display:flex;overflow:hidden">
<div style="width:30%;background:var(--chrome);border-right:1px solid var(--hair);padding:9px 7px;display:flex;flex-direction:column;gap:7px">{bar(80, "var(--accent-tint)", 8)}{bar(60)}{bar(70)}{bar(50)}</div>
<div style="flex:1;background:var(--stage)"></div>
<div style="width:46%;background:var(--surface);border-left:1px solid var(--hair);padding:8px;display:flex;flex-direction:column;gap:9px">{top}{rows}</div></div>"""


def layout_card(kind, label, desc, selected):
    border = "border:2px solid var(--accent)" if selected else "border:1px solid var(--hair);margin:1px"
    return f"""<div style="flex:1;border-radius:10px;background:var(--surface);{border};padding:10px;display:flex;flex-direction:column;gap:8px">
{skeleton(kind)}<div style="text-align:center"><div style="font-weight:{650 if selected else 500};color:{'var(--text)' if selected else 'var(--muted)'}">{label}</div>
<div style="color:var(--faint);font-size:11.5px;margin-top:2px">{desc}</div></div></div>"""


def switch(on):
    bg = "var(--accent)" if on else "var(--raised)"
    pos = "right:2px" if on else "left:2px"
    return f'<span style="width:36px;height:20px;border-radius:10px;background:{bg};position:relative;flex:none"><i style="position:absolute;top:2px;{pos};width:16px;height:16px;border-radius:50%;background:#fff"></i></span>'


def form_row(label, note, control, last=False):
    return f"""<div style="display:flex;align-items:center;gap:16px;padding:10px 0;{'' if last else 'border-bottom:1px solid var(--hair)'}">
<div style="flex:1"><div>{label}</div><div style="color:var(--faint);font-size:11.5px;margin-top:2px">{note}</div></div>{control}</div>"""


def s_settings():
    cats = [("Server", "server", False), ("Appearance", "palette", False), ("Chat", "chat", True), ("Streaming", "share", False), None,
            ("Audio", "wave", False), ("Shortcuts", "kbd", False), ("Permissions", "shield", False), "App",
            ("Updates", "download", False), ("Error Log", "alert", False), ("Settings File", "file", False)]
    side = ""
    for c in cats:
        if c is None:
            side += '<div style="color:var(--faint);font-size:11px;padding:12px 10px 4px">Voice</div>'
        elif isinstance(c, str):
            side += f'<div style="color:var(--faint);font-size:11px;padding:12px 10px 4px">{c}</div>'
        else:
            n, ic, on = c
            st = "background:var(--raised);color:var(--text);font-weight:600" if on else "color:var(--muted)"
            side += f'<div style="height:32px;border-radius:6px;display:flex;align-items:center;gap:10px;padding:0 10px;{st}">{I[ic]}{n}</div>'
    group = lambda inner: f'<div style="border-radius:10px;background:var(--surface);border:1px solid var(--hair);padding:0 14px">{inner}</div>'
    content = f"""
<div style="font-weight:600;margin-bottom:10px">Chat Layout</div>
<div style="display:flex;gap:12px">{layout_card('a', 'Tabs', 'One tab per conversation', True)}{layout_card('b', 'One Feed', 'Everything in one stream', False)}</div>
<div style="color:var(--faint);font-size:11.5px;margin:8px 0 22px">Both show channel, server and private messages in the column beside the stage.</div>
<div style="font-weight:600;margin-bottom:10px">While Chat Is Hidden</div>
{group(form_row('Show new messages', 'A pop-up at the top right for 5 seconds.', switch(True))
       + form_row('Only private messages', 'Channel and server messages just light the dot.', switch(False))
       + form_row('Hide chat while watching a stream', 'Folds it to the edge when a stream fills the stage.', switch(False), last=True))}
"""
    panel = f"""<div style="position:absolute;inset:28px 36px;border-radius:12px;border:1px solid var(--hair);overflow:hidden;display:flex;
box-shadow:0 20px 60px rgba(0,0,0,.55)">
<div style="width:232px;background:var(--chrome);padding:10px;border-right:1px solid var(--hair)">
<div style="height:30px;border-radius:6px;background:var(--recess);border:1px solid var(--hair);display:flex;align-items:center;gap:8px;padding:0 8px;color:var(--faint);margin-bottom:10px">{I['search']}Search</div>{side}</div>
<div style="flex:1;background:var(--canvas);display:flex;flex-direction:column">
<div style="height:52px;display:flex;align-items:center;padding:0 16px 0 20px;gap:10px;border-bottom:1px solid var(--hair)">
<span style="color:var(--muted)">Settings</span><span style="color:var(--faint)">/</span><b>Chat</b><span style="flex:1"></span><span class="btn pri">Save</span><span class="ib">{I['x']}</span></div>
<div style="padding:24px 56px;overflow:hidden">{content}</div></div></div>"""
    backdrop = f'<div style="position:absolute;inset:0;filter:blur(6px);opacity:.35">{window(sidebar(tree()) + stage() + chat_a())}</div>'
    win = f'<div class="win" style="position:relative;background:#0b0c0e">{backdrop}{panel}</div>'
    return win, [
        (270, 120, "1", "A new Chat category, next to Appearance. Same picture cards as Color Scheme."),
        (680, 130, "2", "Tabs or One Feed, chosen in onboarding and changeable here; it switches live."),
        (296, 420, "3", "Pop-ups for the hidden state can be narrowed to private messages or turned off."),
    ]


# ---------- onboarding ----------

def example(kind, selected):
    panel = (chat_a(width=0, collapse=False) if kind == "a" else chat_b(width=0, collapse=False)).replace("border-left:1px solid var(--hair);", "")
    name, desc = (("Tabs", "One tab each for your channel, the server and every person you message.") if kind == "a"
                  else ("One Feed", "Everything in one stream, tagged by where it was sent."))
    ring = "border:2px solid var(--accent)" if selected else "border:1px solid var(--hair);margin:1px"
    tick = (f'<span style="width:18px;height:18px;border-radius:50%;background:var(--accent);color:#14161c;display:grid;place-items:center">{I["check"]}</span>'
            if selected else '<span style="width:18px;height:18px;border-radius:50%;border:1.5px solid var(--faint)"></span>')
    return f"""<div style="flex:1;border-radius:12px;background:var(--surface);{ring};padding:12px;display:flex;flex-direction:column;gap:12px">
<div style="height:318px;border-radius:8px;overflow:hidden;border:1px solid var(--hair);display:flex;background:var(--surface)">{panel}</div>
<div style="display:flex;gap:10px;align-items:flex-start">{tick}<div><div style="font-weight:650">{name}</div>
<div style="color:var(--muted);font-size:12px;margin-top:2px;line-height:1.4">{desc}</div></div></div></div>"""


def s_onboarding():
    steps = ["Your username", "Your server", "Permissions", "Your audio devices", "Your chat", "Your shortcuts"]
    bars = "".join(f"""<div style="flex:1"><div style="height:3px;border-radius:2px;background:{'var(--accent)' if i < 5 else 'var(--raised)'}"></div>
<div style="margin-top:8px;font-size:12px;color:{'var(--accent)' if i < 5 else 'var(--faint)'}">{i + 1}. {n}</div></div>""" for i, n in enumerate(steps))
    win = f"""<div class="win" style="position:relative"><div style="height:22px"></div>
<div style="width:780px;margin:0 auto;display:flex;flex-direction:column;flex:1">
<div style="color:var(--faint);font-weight:600;font-size:12px">Welcome to Flexspeak</div>
<div style="font-size:22px;font-weight:650;margin:10px 0 18px">Let’s get you talking</div>
<div style="display:flex;gap:6px">{bars}</div>
<div style="font-size:15px;font-weight:650;margin:22px 0 6px">Your chat</div>
<div style="color:var(--text);margin-bottom:14px">Pick how messages from your channel, the server and single people show up. You can switch any time in Settings › Chat.</div>
<div style="display:flex;gap:14px">{example('a', True)}{example('b', False)}</div>
<div style="flex:1"></div>
<div style="display:flex;justify-content:space-between;padding:0 0 22px"><span class="btn">Back</span><span class="btn pri">Continue</span></div></div></div>"""
    return win, [
        (130, 140, "1", "A new step 5, before shortcuts. Tabs is preselected, so Continue keeps the familiar layout."),
        (130, 330, "2", "Each card is a real, live chat sample, not a sketch, so people see exactly what they get."),
        (970, 330, "3", "Clicking a card selects it; the same choice sits in Settings › Chat."),
    ]


def page(title, win, notes):
    pins = "".join(f'<div class="pin" style="left:{x}px;top:{y}px">{n}</div>' for x, y, n, _ in notes)
    legend = "".join(f"<div><b>{n}</b><span>{t}</span></div>" for _, _, n, t in notes)
    name, _, idea = title.partition(" — ")
    return f"""<!doctype html><html><head><meta charset="utf-8"><title>{name}</title><style>{CSS}
body{{padding:0 24px;background:#0e0f11}}</style></head><body><div style="width:1120px;padding:18px 0">
<div class="ttl">{name}<span>{idea}</span></div><div class="frame">{win}{pins}</div><div class="legend">{legend}</div></div></body></html>"""


SCREENS = [
    ("01-tabs-stream", "1 · Tabs, watching a stream — chat column open beside the stream", s_tabs_stream),
    ("02-feed-stream", "2 · One Feed, watching a stream — the same column as a single stream", s_feed_stream),
    ("03-hidden", "3 · Chat hidden — a handle on the right edge, like the server rail's", s_collapsed),
    ("04-hidden-new-channel", "4 · Hidden, new channel message — pop-up top right and a blue dot", s_collapsed_channel),
    ("05-hidden-new-private", "5 · Hidden, new private message — violet pop-up and dot", s_collapsed_private),
    ("06-peek", "6 · Hovering the handle — the chat peeks over the stream", s_peek),
    ("07-settings", "7 · Settings › Chat — pick the layout and the hidden-state pop-ups", s_settings),
    ("08-onboarding", "8 · Onboarding step — two live examples to choose from", s_onboarding),
]

if __name__ == "__main__":
    for old in HERE.glob("*.html"):
        old.unlink()
    for slug, title, fn in SCREENS:
        win, notes = fn()
        (HERE / f"{slug}.html").write_text(page(title, win, notes))
    print("wrote", len(SCREENS), "screens")
