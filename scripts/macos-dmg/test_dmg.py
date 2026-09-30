"""Native packaging regression using a signed synthetic app, never user data."""

import plistlib
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from ds_store import DSStore
from mac_alias import Alias

ROOT = Path(__file__).resolve().parents[2]


def run(*args, **kwargs):
    return subprocess.run(
        args, check=True, capture_output=True, timeout=60, **kwargs
    ).stdout


@unittest.skipUnless(sys.platform == "darwin", "requires macOS disk image tools")
class DiskImageLayout(unittest.TestCase):
    def test_signed_app_and_finder_layout(self):
        with tempfile.TemporaryDirectory(prefix="sotto-dmg-test-") as temporary:
            work = Path(temporary)
            app = work / "Sotto.app"
            contents = app / "Contents"
            (contents / "MacOS").mkdir(parents=True)
            (contents / "Info.plist").write_bytes(
                plistlib.dumps(
                    {
                        "CFBundleIdentifier": "com.sotto.dmg-layout-check",
                        "CFBundleExecutable": "Sotto",
                        "CFBundlePackageType": "APPL",
                    }
                )
            )
            run(
                "cc",
                "-x",
                "c",
                "-o",
                str(contents / "MacOS/Sotto"),
                "-",
                input=b"int main(void) { return 0; }\n",
            )
            run("codesign", "--force", "--sign", "-", str(app))
            image = work / "layout.dmg"
            run("sh", str(ROOT / "scripts/build-dmg.sh"), str(app), str(image))
            mount = work / "mount"
            mount.mkdir()
            run(
                "hdiutil",
                "attach",
                "-readonly",
                "-nobrowse",
                "-noautoopen",
                "-mountpoint",
                str(mount),
                str(image),
                input=b"Y\n",
            )
            try:
                self.assertEqual(
                    {
                        path.name
                        for path in mount.iterdir()
                        if not path.name.startswith(".")
                    },
                    {"Sotto.app", "Applications"},
                )
                self.assertEqual(
                    (mount / "Applications").readlink(), Path("/Applications")
                )
                run("codesign", "--verify", "--strict", str(mount / "Sotto.app"))
                for name in (".background.tiff", ".VolumeIcon.icns"):
                    info = run(
                        "xattr", "-px", "com.apple.FinderInfo", str(mount / name)
                    )
                    flags = int.from_bytes(bytes.fromhex(info.decode())[8:10], "big")
                    self.assertTrue(flags & 0x4000, f"{name} must be invisible")

                with DSStore.open(str(mount / ".DS_Store"), "r") as store:
                    self.assertEqual(store["."]["icvl"], (b"type", b"icnv"))
                    window = store["."]["bwsp"]
                    self.assertTrue(window["WindowBounds"].endswith("{660, 400}}"))
                    for key in (
                        "ShowToolbar",
                        "ShowSidebar",
                        "ShowStatusBar",
                        "ShowPathbar",
                        "ShowTabView",
                    ):
                        self.assertFalse(window[key], key)
                    view = store["."]["icvp"]
                    self.assertLess(view["gridSpacing"], 100)
                    self.assertEqual(view["arrangeBy"], "none")
                    self.assertEqual(view["iconSize"], 100)
                    self.assertEqual(view["scrollPositionX"], 0)
                    self.assertEqual(view["scrollPositionY"], 0)
                    self.assertEqual(view["backgroundType"], 2)
                    alias = Alias.from_bytes(view["backgroundImageAlias"])
                    self.assertEqual(alias.target.filename, ".background.tiff")
                    self.assertEqual(store["Sotto.app"]["Iloc"], (180, 185))
                    self.assertEqual(store["Applications"]["Iloc"], (480, 185))

                dimensions = run(
                    "sips",
                    "-g",
                    "pixelWidth",
                    "-g",
                    "pixelHeight",
                    str(mount / ".background.tiff"),
                ).decode()
                self.assertEqual(re.search(r"pixelWidth: (\d+)", dimensions)[1], "660")
                self.assertEqual(re.search(r"pixelHeight: (\d+)", dimensions)[1], "400")
            finally:
                run("hdiutil", "detach", str(mount))


if __name__ == "__main__":
    unittest.main()
