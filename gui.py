from __future__ import annotations

import tkinter as tk
from tkinter import filedialog, messagebox, ttk

from engine import scan_path
from utils import dumps_json, write_jsonl


class MetaXtractGUI(tk.Tk):
    def __init__(self) -> None:
        super().__init__()
        self.title("MetaXtract")
        self.geometry("900x600")

        top = ttk.Frame(self)
        top.pack(side=tk.TOP, fill=tk.X, padx=10, pady=10)

        self.path_var = tk.StringVar(value="")
        ttk.Label(top, text="Target").pack(side=tk.LEFT)
        ttk.Entry(top, textvariable=self.path_var, width=60).pack(side=tk.LEFT, padx=8)
        ttk.Button(top, text="Browse", command=self._browse).pack(side=tk.LEFT)
        ttk.Button(top, text="Scan", command=self._scan).pack(side=tk.LEFT, padx=8)
        ttk.Button(top, text="Export JSONL", command=self._export).pack(side=tk.LEFT)

        self.text = tk.Text(self, wrap="none")
        self.text.pack(side=tk.TOP, fill=tk.BOTH, expand=True, padx=10, pady=10)

        self._last_records = None

    def _browse(self) -> None:
        p = filedialog.askopenfilename()
        if not p:
            p = filedialog.askdirectory()
        if p:
            self.path_var.set(p)

    def _scan(self) -> None:
        target = self.path_var.get().strip()
        if not target:
            messagebox.showwarning("MetaXtract", "Select a file or folder first.")
            return

        records = scan_path(target)
        self._last_records = records

        self.text.delete("1.0", tk.END)
        for r in records:
            self.text.insert(tk.END, dumps_json(r) + "\n")

    def _export(self) -> None:
        if not self._last_records:
            messagebox.showwarning("MetaXtract", "Run a scan first.")
            return
        out = filedialog.asksaveasfilename(
            defaultextension=".jsonl",
            filetypes=[("JSONL", "*.jsonl")],
        )
        if not out:
            return
        write_jsonl(out, self._last_records)
        messagebox.showinfo("MetaXtract", f"Saved: {out}")


def main() -> None:
    app = MetaXtractGUI()
    app.mainloop()


if __name__ == "__main__":
    main()
