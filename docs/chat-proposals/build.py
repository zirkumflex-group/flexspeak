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


# ---------- Style A: Tabs ----------

def style_a():
    tabs = f"""<div style="display:flex;align-items:center;gap:2px;padding:8px 8px 0;border-bottom:1px solid var(--hair);height:41px">
<div style="height:32px;padding:0 10px;display:flex;align-items:center;gap:6px;color:var(--accent);font-weight:600;border-bottom:2px solid var(--accent)">{I['hash']}Lobby</div>
<div style="height:32px;padding:0 10px;display:flex;align-items:center;gap:6px;color:var(--muted)">{I['globe']}Server</div>
<div style="height:32px;padding:0 8px 0 10px;display:flex;align-items:center;gap:6px;color:var(--text)"><span class="av c-mara" style="width:16px;height:16px;font-size:9px">M</span>Mara<span class="badge dm">2</span></div>
<div style="height:32px;padding:0 8px 0 10px;display:flex;align-items:center;gap:6px;color:var(--muted)"><span class="av c-ines" style="width:16px;height:16px;font-size:9px">I</span>Ines</div>
<div style="flex:1"></div><div class="ib">{I['plus']}</div></div>"""
    msgs = "".join([
        '<div class="sysline">Today</div>',
        msg("Theo", "c-theo", "21:04", "back, had to restart my router"),
        msg("Mara", "c-mara", "21:05", "we start the raid at half past, Ines is setting up"),
        msg("Jan", "c-jan", "21:06", "👍 I'll join after this match"),
        msg("Theo", "c-theo", "21:07", "can someone share the boss notes link again?"),
    ])
    chat = f"""<div class="chat" style="width:350px;border-left:1px solid var(--hair)">{tabs}
<div style="padding:10px 14px 0;color:var(--faint);font-size:11.5px;display:flex;align-items:center;gap:6px">{I['hash']}<span>Everyone in <b style="color:var(--muted)">Lobby</b> sees this</span></div>
<div class="msgs">{msgs}</div><div class="typing">Mara is typing…</div>
<div class="compose"><span class="ph">Message #Lobby</span><span class="ib send">{I['send']}</span></div></div>"""
    win = f"""<div class="win">{header()}<div class="body">{sidebar(tree(mem_extra={"Mara": '<span class="badge dm">2</span>'}))}
<div class="stage">{stage()}</div>{chat}</div>{footer()}</div>"""
    notes = [
        (758, 64, "1", "One tab per conversation: channel, server, then one per person. The open tab is where you're typing."),
        (758, 614, "2", "The box says the target: Message #Lobby, Message Mara."),
        (12, 159, "3", "An unread private message shows on the tab and on the person in the tree."),
    ]
    return win, notes


# ---------- Style B: One feed ----------

def style_b():
    filters = f"""<div style="display:flex;align-items:center;gap:6px;padding:10px 12px;border-bottom:1px solid var(--hair)">
<span class="chip sel">All</span><span class="chip plain">{I['hash'].replace('<svg>','<svg class="sm">')}Lobby</span>
<span class="chip plain">{I['globe'].replace('<svg>','<svg class="sm">')}Server</span><span class="chip plain">{I['user'].replace('<svg>','<svg class="sm">')}Private <span class="badge dm" style="height:14px;min-width:14px;font-size:9.5px">2</span></span>
<span style="flex:1"></span></div>"""
    edge = lambda c: f"border-left:2px solid {c};padding-left:8px;margin-left:-10px"
    dm_box = "background:var(--dm-tint);border-radius:8px;padding:8px 10px;margin:0 -4px"
    msgs = "".join([
        msg("Lukas", "c-lu", "20:58", "Server restarts at 4am for the update", f'<span class="chip srv">{I["globe"].replace("<svg>","<svg class=sm>")}Server</span>'),
        msg("Theo", "c-theo", "21:04", "back, had to restart my router", '<span class="chip ch">#Lobby</span>'),
        msg("Mara", "c-mara", "21:05", "we start the raid at half past", '<span class="chip ch">#Lobby</span>'),
        msg("Mara", "c-mara", "21:06", "psst, can you bring the spare key for the vault run?",
            f'<span class="chip dm">{I["user"].replace("<svg>","<svg class=sm>")}to you</span>', dm_box),
        msg("Jan", "c-jan", "21:06", "yes, got it 👍",
            f'<span class="chip dm">{I["user"].replace("<svg>","<svg class=sm>")}to Mara</span>', dm_box),
        msg("Theo", "c-theo", "21:07", "can someone share the boss notes link again?", '<span class="chip ch">#Lobby</span>'),
    ])
    compose = f"""<div class="compose" style="padding-left:6px"><span class="chip dm" style="height:24px;border-radius:6px">{I['user'].replace('<svg>','<svg class="sm">')}To Mara {I['chev'].replace('chev','')}</span>
<span class="ph">Reply privately…</span><span class="ib" style="color:var(--faint)">{I['x']}</span><span class="ib send">{I['send']}</span></div>"""
    chat = f"""<div class="chat" style="width:360px;border-left:1px solid var(--hair)">{filters}
<div class="msgs">{msgs}</div><div class="typing">Esc goes back to #Lobby</div>{compose}</div>"""
    win = f"""<div class="win">{header()}<div class="body">{sidebar(tree())}
<div class="stage">{stage()}</div>{chat}</div>{footer()}</div>"""
    notes = [
        (748, 66, "1", "Everything in one stream, like a game chat. Filters only narrow it, they never hide where you type."),
        (748, 430, "2", "Private lines get a tint and to you / to Mara, so they can't pass for channel talk."),
        (748, 614, "3", "Replying to a private line points the box at that person. Esc or ✕ goes back to the channel."),
    ]
    return win, notes


# ---------- Style C: Talk to what you select ----------

def style_c():
    tree_html = tree(
        lobby_extra='<span class="badge quiet">3</span>',
        raid_extra="",
        mem_extra={"Mara": f'<span class="badge dm">2</span>'},
        sel={"Mara"},
    )
    head = f"""<div style="height:52px;display:flex;align-items:center;gap:10px;padding:0 16px;border-bottom:1px solid var(--hair)">
<span class="btn ghost" style="padding:0 8px">{I['arrowl']}<span style="color:var(--accent)">#Lobby</span><span class="badge quiet">3</span></span>
<span style="color:var(--faint)">/</span>
<span class="av c-mara" style="width:22px;height:22px">M</span><b style="font-size:14px">Mara</b><span class="chip dm">Private</span>
<span style="flex:1"></span><span style="color:var(--faint);font-size:11.5px">In Lobby with you</span></div>"""
    dm_msgs = "".join([
        '<div class="sysline">Only you and Mara see this</div>',
        msg("Mara", "c-mara", "21:06", "psst, can you bring the spare key for the vault run?"),
        msg("Mara", "c-mara", "21:06", "and don't tell Theo yet 😄"),
        msg("Jan", "c-jan", "21:08", "haha ok, I'll grab it before we start"),
    ])
    chat = f"""<div class="chat" style="flex:1;background:var(--stage);border-top:1px solid var(--hair)">{head}
<div class="msgs" style="padding:12px 20px">{dm_msgs}</div>
<div class="compose" style="margin:0 20px 14px;border-color:rgba(201,166,255,.35)"><span class="ph">Message Mara privately</span><span class="ib send">{I['send']}</span></div></div>"""
    stage_html = f"""<div class="stage">{stage(compact=True)}{chat}</div>"""
    win = f"""<div class="win">{header()}<div class="body">{sidebar(tree_html, with_log=True)}{stage_html}</div>{footer()}</div>"""
    notes = [
        (12, 159, "1", "Click a channel to chat there, click a person to chat with them. Unread counts sit on the tree rows."),
        (975, 200, "2", "One chat pane under the stage. The header shows which conversation it is and leads back to the channel."),
        (268, 614, "3", "Private chats get the violet edge and a Private tag everywhere."),
    ]
    return win, notes


def page(title, win, notes, standalone=True):
    pins = "".join(f'<div class="pin" style="left:{x}px;top:{y}px">{n}</div>' for x, y, n, _ in notes)
    legend = "".join(f"<div><b>{n}</b><span>{t}</span></div>" for _, _, n, t in notes)
    name, _, idea = title.partition(" — ")
    frame = (f'<div style="width:1120px;padding:18px 0"><div class="ttl">{name}<span>{idea}</span></div>'
             f'<div class="frame">{win}{pins}</div><div class="legend">{legend}</div></div>')
    if not standalone:
        return frame
    return f"""<!doctype html><html><head><meta charset="utf-8"><title>{title}</title><style>{CSS}
body{{padding:0 24px;background:#0e0f11}}</style></head><body>{frame}</body></html>"""


STYLES = [
    ("a-tabs", "A · Tabs — a chat column with one tab per conversation", style_a),
    ("b-one-feed", "B · One feed — channel, server and private messages in one stream", style_b),
    ("c-select-to-chat", "C · Select to chat — the channel tree is the conversation list", style_c),
]

if __name__ == "__main__":
    for slug, title, fn in STYLES:
        win, notes = fn()
        (HERE / f"{slug}.html").write_text(page(title, win, notes))
    print("wrote", ", ".join(s for s, _, _ in STYLES))
