from __future__ import annotations

import base64
import tkinter as tk
from importlib.resources import files


ICON_SIZES = (16, 32, 48, 64, 128, 256)


def apply_window_icons(window: tk.Tk | tk.Toplevel) -> list[tk.PhotoImage]:
    icons = []
    assets = files("metaxtract").joinpath("assets")
    for size in ICON_SIZES:
        try:
            image_data = assets.joinpath(f"metaxtract_{size}.png").read_bytes()
            encoded = base64.b64encode(image_data).decode("ascii")
            icons.append(tk.PhotoImage(data=encoded, format="png"))
        except (OSError, tk.TclError):
            continue
    if icons:
        window.iconphoto(True, *icons)
    return icons
