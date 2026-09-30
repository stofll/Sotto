"""dmgbuild layout for the Sotto disk image; scripts/build-dmg.sh passes the paths.

dmgbuild writes the Finder window settings into .DS_Store itself, so the layout
does not depend on Finder automation, which drops the background and icon size
on current macOS and cannot run on a headless CI runner.
"""

# ruff: noqa: F821 -- dmgbuild injects `defines` from its -D arguments.

import os.path

app = defines["app"]

format = "UDZO"
filesystem = "HFS+"
files = [app]
symlinks = {"Applications": "/Applications"}
icon = defines["volume_icon"]
license = {
    "default-language": "en_US",
    "licenses": {"en_US": defines["license"]},
}

# Finder always draws icon labels in black, so the artwork stays light.
background = defines["background"]
hide = [".background.tiff", ".VolumeIcon.icns"]
window_rect = ((200, 120), (660, 400))
default_view = "icon-view"
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False

# Positions are icon centres and match the chevrons in
# desktop/src-tauri/installer/macos/background.svg.
icon_size = 100
text_size = 13
arrange_by = None
# Finder rejects icon-view settings with the dmgbuild default of 100.
grid_spacing = 80
icon_locations = {
    os.path.basename(app): (180, 185),
    "Applications": (480, 185),
}
