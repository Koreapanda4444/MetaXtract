from __future__ import annotations

import queue
import threading
import tkinter as tk
from tkinter import filedialog, messagebox, ttk
from typing import Any

from .core.jsonio import dumps_json, write_jsonl
from .core.scanner import ScanCancelled, scan_path


class MetaXtractGUI(tk.Tk):
    def __init__(self) -> None:
        super().__init__()
        self.title("MetaXtract")
        self.geometry("900x600")

        top = ttk.Frame(self)
        top.pack(side=tk.TOP, fill=tk.X, padx=10, pady=10)

        self.path_var = tk.StringVar(value="")
        ttk.Label(top, text="Target").pack(side=tk.LEFT)
        self.path_entry = ttk.Entry(top, textvariable=self.path_var, width=52)
        self.path_entry.pack(side=tk.LEFT, padx=8)
        self.browse_button = ttk.Button(top, text="Browse", command=self._browse)
        self.browse_button.pack(side=tk.LEFT)
        self.scan_button = ttk.Button(top, text="Scan", command=self._scan)
        self.scan_button.pack(side=tk.LEFT, padx=(8, 4))
        self.cancel_button = ttk.Button(
            top,
            text="Cancel",
            command=self._cancel_scan,
            state=tk.DISABLED,
        )
        self.cancel_button.pack(side=tk.LEFT, padx=(0, 8))
        self.export_button = ttk.Button(
            top,
            text="Export JSONL",
            command=self._export,
            state=tk.DISABLED,
        )
        self.export_button.pack(side=tk.LEFT)

        progress_frame = ttk.Frame(self)
        progress_frame.pack(side=tk.TOP, fill=tk.X, padx=10)
        self.progress = ttk.Progressbar(progress_frame, mode="determinate")
        self.progress.pack(side=tk.TOP, fill=tk.X)
        self.status_var = tk.StringVar(value="Ready")
        ttk.Label(progress_frame, textvariable=self.status_var).pack(
            side=tk.TOP,
            anchor=tk.W,
            pady=(4, 0),
        )

        self.text = tk.Text(self, wrap="none")
        self.text.pack(side=tk.TOP, fill=tk.BOTH, expand=True, padx=10, pady=10)

        self._last_records = None
        self._events: queue.Queue[tuple[str, Any]] = queue.Queue()
        self._cancel_event: threading.Event | None = None
        self._worker: threading.Thread | None = None
        self.protocol("WM_DELETE_WINDOW", self._close)
        self.after(50, self._poll_events)

    def _browse(self) -> None:
        p = filedialog.askopenfilename()
        if not p:
            p = filedialog.askdirectory()
        if p:
            self.path_var.set(p)

    def _scan(self) -> None:
        if self._worker is not None:
            return
        target = self.path_var.get().strip()
        if not target:
            messagebox.showwarning("MetaXtract", "Select a file or folder first.")
            return

        self._last_records = None
        self.text.delete("1.0", tk.END)
        self.progress.configure(maximum=1, value=0)
        self.status_var.set("Discovering files…")
        self._set_scanning(True)
        self._cancel_event = threading.Event()
        self._worker = threading.Thread(
            target=self._scan_worker,
            args=(target, self._cancel_event),
            name="metaxtract-scan",
            daemon=True,
        )
        self._worker.start()

    def _scan_worker(self, target: str, cancel_event: threading.Event) -> None:
        def report_progress(completed: int, total: int, path: str | None) -> None:
            self._events.put(("progress", (completed, total, path)))

        try:
            records = scan_path(
                target,
                progress_callback=report_progress,
                cancel_check=cancel_event.is_set,
            )
        except ScanCancelled:
            self._events.put(("cancelled", None))
        except Exception as exc:
            self._events.put(("error", (type(exc).__name__, str(exc))))
        else:
            self._events.put(("complete", records))

    def _cancel_scan(self) -> None:
        if self._cancel_event is None:
            return
        self._cancel_event.set()
        self.cancel_button.configure(state=tk.DISABLED)
        self.status_var.set("Cancelling…")

    def _poll_events(self) -> None:
        try:
            while True:
                event, payload = self._events.get_nowait()
                if event == "progress":
                    completed, total, path = payload
                    self.progress.configure(maximum=max(total, 1), value=completed)
                    if path is None:
                        self.status_var.set(f"Scanning 0/{total}")
                    else:
                        self.status_var.set(f"Scanning {completed}/{total}: {path}")
                elif event == "complete":
                    self._finish_scan(payload)
                elif event == "cancelled":
                    self._finish_scan(None, "Scan cancelled.")
                elif event == "error":
                    error_type, detail = payload
                    self._finish_scan(None, "Scan failed.")
                    messagebox.showerror(
                        "MetaXtract",
                        f"{error_type}: {detail}",
                    )
        except queue.Empty:
            pass
        if self.winfo_exists():
            self.after(50, self._poll_events)

    def _finish_scan(self, records, status: str | None = None) -> None:
        self._worker = None
        self._cancel_event = None
        self._set_scanning(False)
        if records is None:
            self.status_var.set(status or "Scan failed.")
            return

        self._last_records = records
        self.progress.configure(maximum=max(len(records), 1), value=len(records))
        self.status_var.set(f"Completed: {len(records)} file(s).")

        for record in records:
            self.text.insert(tk.END, dumps_json(record) + "\n")
        self.export_button.configure(state=tk.NORMAL)

    def _set_scanning(self, scanning: bool) -> None:
        idle_state = tk.DISABLED if scanning else tk.NORMAL
        self.path_entry.configure(state=idle_state)
        self.browse_button.configure(state=idle_state)
        self.scan_button.configure(state=idle_state)
        self.export_button.configure(state=tk.DISABLED)
        self.cancel_button.configure(state=tk.NORMAL if scanning else tk.DISABLED)

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
        try:
            write_jsonl(out, self._last_records)
        except (OSError, ValueError) as exc:
            messagebox.showerror("MetaXtract", f"Export failed: {exc}")
            return
        messagebox.showinfo("MetaXtract", f"Saved: {out}")

    def _close(self) -> None:
        if self._cancel_event is not None:
            self._cancel_event.set()
        self.destroy()


def main() -> None:
    app = MetaXtractGUI()
    app.mainloop()


if __name__ == "__main__":
    main()
