from __future__ import annotations

import base64
import tkinter as tk
from tkinter import ttk

from metaxtract.core.preview import PreviewContent
from metaxtract.ui.icons import apply_window_icons
from metaxtract.ui.records import format_bytes


class PreviewDialog(tk.Toplevel):
    def __init__(self, parent: tk.Misc, preview: PreviewContent) -> None:
        super().__init__(parent)
        self.title(f"Preview — {preview.title}")
        self.geometry("1000x760")
        self.minsize(640, 420)
        self.transient(parent)
        self._window_icons = apply_window_icons(self)
        self._preview_image: tk.PhotoImage | None = None

        self.columnconfigure(0, weight=1)
        self.rowconfigure(2, weight=1)
        ttk.Label(
            self,
            text=f"{preview.source_path} · {format_bytes(preview.source_size)}",
            padding=(10, 10, 10, 4),
        ).grid(row=0, column=0, sticky=tk.W)
        ttk.Label(
            self,
            text=preview.integrity,
            padding=(10, 0, 10, 8),
            foreground="#b42318" if preview.integrity.startswith("Warning") else "#087f23",
        ).grid(row=1, column=0, sticky=tk.W)

        content = ttk.Frame(self)
        content.grid(row=2, column=0, sticky=tk.NSEW, padx=10)
        content.columnconfigure(0, weight=1)
        content.rowconfigure(0, weight=1)
        if preview.kind == "image" and preview.image_png is not None:
            encoded = base64.b64encode(preview.image_png).decode("ascii")
            self._preview_image = tk.PhotoImage(data=encoded, format="png")
            image_label = ttk.Label(content, image=self._preview_image, anchor=tk.CENTER)
            image_label.grid(row=0, column=0, sticky=tk.NSEW)
            ttk.Label(content, text=preview.text, padding=(0, 8, 0, 0)).grid(
                row=1,
                column=0,
                sticky=tk.W,
            )
        else:
            text = tk.Text(content, wrap="word", state=tk.NORMAL)
            scroll = ttk.Scrollbar(content, orient=tk.VERTICAL, command=text.yview)
            text.configure(yscrollcommand=scroll.set)
            text.grid(row=0, column=0, sticky=tk.NSEW)
            scroll.grid(row=0, column=1, sticky=tk.NS)
            text.insert("1.0", preview.text)
            text.configure(state=tk.DISABLED)

        actions = ttk.Frame(self, padding=10)
        actions.grid(row=3, column=0, sticky=tk.E)
        ttk.Button(actions, text="Close", command=self.destroy).pack()
        self.bind("<Escape>", lambda _event: self.destroy())


def show_preview(parent: tk.Misc, preview: PreviewContent) -> None:
    dialog = PreviewDialog(parent, preview)
    dialog.grab_set()
    dialog.focus_set()
    parent.wait_window(dialog)
