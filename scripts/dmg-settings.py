"""Finder layout for dmgbuild; no Finder automation or logged-in GUI is required."""

from pathlib import Path

application = Path(defines["app"])
format = "UDZO"
filesystem = "HFS+"
files = [str(application)]
symlinks = {"Applications": "/Applications"}
badge_icon = str(application / "Contents/Resources/Flexspeak.icns")
background = defines["background"]
window_rect = ((200, 160), (640, 400))
icon_locations = {"Flexspeak.app": (170, 190), "Applications": (470, 190)}
icon_size = 96
text_size = 14
default_view = "icon-view"
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
include_icon_view_settings = True
include_list_view_settings = False
arrange_by = None
