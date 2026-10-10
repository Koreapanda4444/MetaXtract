from __future__ import annotations

import json
import queue
import threading
import tkinter as tk
from pathlib import Path
from tkinter import filedialog, messagebox, ttk
from typing import Any

from metaxtract.ui import dialogs
from metaxtract.ui.icons import apply_window_icons
from metaxtract.ui.records import (
    FILTER_ALL,
    FILTER_VALUES,
    filter_records,
    format_bytes,
    record_dict,
    record_status,
)
from metaxtract.ui.tasks import run_scan, run_verification


class MetaXtractGUI(tk.Tk):
    def __init__(self) -> None:
        super().__init__()
        self.title("MetaXtract")
        self._window_icons = apply_window_icons(self)
        self.geometry("1120x720")
        self.minsize(860, 560)

        self.path_var = tk.StringVar(value="")
        self.cache_var = tk.BooleanVar(value=True)
        self.hidden_var = tk.BooleanVar(value=False)
        self.max_files_var = tk.StringVar(value="5000")
        self.search_var = tk.StringVar(value="")
        self.status_filter_var = tk.StringVar(value=FILTER_ALL)
        self.status_var = tk.StringVar(value="Ready")

        self._last_records: list[dict[str, Any]] = []
        self._visible_records: dict[str, dict[str, Any]] = {}
        self._scan_base: Path | None = None
        self._events: queue.Queue[tuple[str, Any]] = queue.Queue()
        self._cancel_event: threading.Event | None = None
        self._worker: threading.Thread | None = None

        self._build_target_controls()
        self._build_progress_controls()
        self._build_filter_controls()
        self._build_results()

        self.search_var.trace_add("write", lambda *_args: self._refresh_table())
        self.protocol("WM_DELETE_WINDOW", self._close)
        self.after(50, self._poll_events)

    def _build_target_controls(self) -> None:
        target = ttk.Frame(self, padding=(10, 10, 10, 4))
        target.pack(side=tk.TOP, fill=tk.X)
        target.columnconfigure(1, weight=1)

        ttk.Label(target, text="Target").grid(row=0, column=0, sticky=tk.W)
        self.path_entry = ttk.Entry(target, textvariable=self.path_var)
        self.path_entry.grid(row=0, column=1, sticky=tk.EW, padx=(8, 6))
        self.file_button = ttk.Button(target, text="Browse File", command=self._browse_file)
        self.file_button.grid(row=0, column=2, padx=2)
        self.folder_button = ttk.Button(
            target,
            text="Browse Folder",
            command=self._browse_folder,
        )
        self.folder_button.grid(row=0, column=3, padx=2)
        self.scan_button = ttk.Button(target, text="Scan", command=self._scan)
        self.scan_button.grid(row=0, column=4, padx=(8, 2))
        self.cancel_button = ttk.Button(
            target,
            text="Cancel",
            command=self._cancel_scan,
            state=tk.DISABLED,
        )
        self.cancel_button.grid(row=0, column=5, padx=(2, 0))

        options = ttk.Frame(target)
        options.grid(row=1, column=1, columnspan=5, sticky=tk.W, pady=(8, 0))
        self.cache_check = ttk.Checkbutton(
            options,
            text="Use cache",
            variable=self.cache_var,
        )
        self.cache_check.pack(side=tk.LEFT)
        self.hidden_check = ttk.Checkbutton(
            options,
            text="Include hidden",
            variable=self.hidden_var,
        )
        self.hidden_check.pack(side=tk.LEFT, padx=(14, 0))
        ttk.Label(options, text="Max files").pack(side=tk.LEFT, padx=(14, 5))
        self.max_files_spin = ttk.Spinbox(
            options,
            from_=1,
            to=1_000_000,
            textvariable=self.max_files_var,
            width=9,
        )
        self.max_files_spin.pack(side=tk.LEFT)

    def _build_progress_controls(self) -> None:
        progress_frame = ttk.Frame(self, padding=(10, 4))
        progress_frame.pack(side=tk.TOP, fill=tk.X)
        self.progress = ttk.Progressbar(progress_frame, mode="determinate")
        self.progress.pack(side=tk.TOP, fill=tk.X)
        ttk.Label(progress_frame, textvariable=self.status_var).pack(
            side=tk.TOP,
            anchor=tk.W,
            pady=(4, 0),
        )

    def _build_filter_controls(self) -> None:
        filters = ttk.Frame(self, padding=(10, 5))
        filters.pack(side=tk.TOP, fill=tk.X)
        filters.columnconfigure(1, weight=1)

        ttk.Label(filters, text="Search").grid(row=0, column=0, sticky=tk.W)
        self.search_entry = ttk.Entry(filters, textvariable=self.search_var)
        self.search_entry.grid(row=0, column=1, sticky=tk.EW, padx=(6, 12))
        ttk.Label(filters, text="Status").grid(row=0, column=2)
        self.status_filter = ttk.Combobox(
            filters,
            textvariable=self.status_filter_var,
            values=FILTER_VALUES,
            state="readonly",
            width=10,
        )
        self.status_filter.grid(row=0, column=3, padx=(6, 12))
        self.status_filter.bind("<<ComboboxSelected>>", lambda _event: self._refresh_table())

        actions = ttk.Frame(filters)
        actions.grid(row=1, column=0, columnspan=4, sticky=tk.W, pady=(7, 0))
        self.jsonl_button = ttk.Button(
            actions,
            text="JSONL",
            command=self._export_jsonl,
            state=tk.DISABLED,
        )
        self.jsonl_button.pack(side=tk.LEFT, padx=(0, 4))
        self.report_json_button = ttk.Button(
            actions,
            text="Report JSON",
            command=lambda: self._export_report("json"),
            state=tk.DISABLED,
        )
        self.report_json_button.pack(side=tk.LEFT, padx=4)
        self.report_html_button = ttk.Button(
            actions,
            text="Report HTML",
            command=lambda: self._export_report("html"),
            state=tk.DISABLED,
        )
        self.report_html_button.pack(side=tk.LEFT, padx=4)
        self.case_button = ttk.Button(
            actions,
            text="Case ZIP",
            command=self._export_case,
            state=tk.DISABLED,
        )
        self.case_button.pack(side=tk.LEFT, padx=4)
        ttk.Separator(actions, orient=tk.VERTICAL).pack(
            side=tk.LEFT,
            fill=tk.Y,
            padx=8,
        )
        self.key_button = ttk.Button(
            actions,
            text="Create Keys",
            command=self._generate_keypair,
        )
        self.key_button.pack(side=tk.LEFT, padx=4)
        self.verify_button = ttk.Button(
            actions,
            text="Verify ZIP",
            command=self._verify_case,
        )
        self.verify_button.pack(side=tk.LEFT, padx=4)

    def _build_results(self) -> None:
        pane = ttk.Panedwindow(self, orient=tk.HORIZONTAL)
        pane.pack(side=tk.TOP, fill=tk.BOTH, expand=True, padx=10, pady=(4, 10))

        table_frame = ttk.Frame(pane)
        details_frame = ttk.Frame(pane)
        pane.add(table_frame, weight=3)
        pane.add(details_frame, weight=2)

        columns = ("path", "mime", "size", "status")
        self.tree = ttk.Treeview(table_frame, columns=columns, show="headings")
        self.tree.heading("path", text="Path")
        self.tree.heading("mime", text="MIME")
        self.tree.heading("size", text="Size")
        self.tree.heading("status", text="Status")
        self.tree.column("path", width=320, minwidth=140)
        self.tree.column("mime", width=220, minwidth=130)
        self.tree.column("size", width=90, minwidth=70, anchor=tk.E)
        self.tree.column("status", width=80, minwidth=70, anchor=tk.CENTER)
        table_scroll = ttk.Scrollbar(
            table_frame,
            orient=tk.VERTICAL,
            command=self.tree.yview,
        )
        self.tree.configure(yscrollcommand=table_scroll.set)
        self.tree.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
        table_scroll.pack(side=tk.RIGHT, fill=tk.Y)
        self.tree.bind("<<TreeviewSelect>>", self._show_selected_record)

        ttk.Label(details_frame, text="Record details").pack(anchor=tk.W, pady=(0, 4))
        detail_container = ttk.Frame(details_frame)
        detail_container.pack(fill=tk.BOTH, expand=True)
        self.details = tk.Text(detail_container, wrap="none", state=tk.DISABLED)
        detail_y = ttk.Scrollbar(
            detail_container,
            orient=tk.VERTICAL,
            command=self.details.yview,
        )
        detail_x = ttk.Scrollbar(
            details_frame,
            orient=tk.HORIZONTAL,
            command=self.details.xview,
        )
        self.details.configure(
            yscrollcommand=detail_y.set,
            xscrollcommand=detail_x.set,
        )
        self.details.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
        detail_y.pack(side=tk.RIGHT, fill=tk.Y)
        detail_x.pack(fill=tk.X)

    def _browse_file(self) -> None:
        selected = filedialog.askopenfilename()
        if selected:
            self.path_var.set(selected)

    def _browse_folder(self) -> None:
        selected = filedialog.askdirectory()
        if selected:
            self.path_var.set(selected)

    def _scan(self) -> None:
        if self._worker is not None:
            return
        target_text = self.path_var.get().strip()
        if not target_text:
            messagebox.showwarning("MetaXtract", "Select a file or folder first.")
            return
        try:
            max_files = int(self.max_files_var.get())
            if max_files < 1:
                raise ValueError
        except ValueError:
            messagebox.showwarning("MetaXtract", "Max files must be a positive integer.")
            return

        target = Path(target_text).expanduser()
        self._scan_base = target.parent if target.is_file() else target
        self._last_records = []
        self._visible_records = {}
        self._clear_results()
        self.progress.configure(maximum=1, value=0)
        self.status_var.set("Discovering files…")
        self._set_busy(True, cancellable=True)
        self._cancel_event = threading.Event()
        self._worker = threading.Thread(
            target=run_scan,
            args=(
                self._events,
                str(target),
                self.cache_var.get(),
                self.hidden_var.get(),
                max_files,
                self._cancel_event,
            ),
            name="metaxtract-scan",
            daemon=True,
        )
        self._worker.start()

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
                elif event == "bytes":
                    self._show_byte_progress(*payload)
                elif event == "complete":
                    self._finish_scan(payload)
                elif event == "cancelled":
                    self._finish_scan(None, "Scan cancelled.")
                elif event == "error":
                    error_type, detail = payload
                    self._finish_scan(None, "Scan failed.")
                    messagebox.showerror("MetaXtract", f"{error_type}: {detail}")
                elif event == "verify_complete":
                    bundle, issues = payload
                    self._finish_verification(bundle, issues)
                elif event == "verify_error":
                    error_type, detail = payload
                    self._finish_verification(None, None)
                    messagebox.showerror(
                        "MetaXtract",
                        f"Verification failed: {error_type}: {detail}",
                    )
        except queue.Empty:
            pass
        if self.winfo_exists():
            self.after(50, self._poll_events)

    def _show_byte_progress(
        self,
        file_index: int,
        total_files: int,
        path: str,
        processed: int,
        total_bytes: int,
    ) -> None:
        fraction = processed / total_bytes if total_bytes else 1.0
        self.progress.configure(
            maximum=max(total_files, 1),
            value=max(file_index - 1, 0) + fraction,
        )
        percent = fraction * 100
        self.status_var.set(
            f"Scanning {file_index}/{total_files}: {path} — "
            f"{format_bytes(processed)} / {format_bytes(total_bytes)} ({percent:.0f}%)"
        )

    def _finish_scan(self, records: Any, status: str | None = None) -> None:
        self._worker = None
        self._cancel_event = None
        self._set_busy(False)
        if records is None:
            self.status_var.set(status or "Scan failed.")
            return

        self._last_records = [record_dict(record) for record in records]
        self.progress.configure(
            maximum=max(len(self._last_records), 1),
            value=len(self._last_records),
        )
        self.status_var.set(f"Completed: {len(self._last_records)} file(s).")
        self._refresh_table()

    def _set_busy(self, busy: bool, *, cancellable: bool = False) -> None:
        idle_state = tk.DISABLED if busy else tk.NORMAL
        for widget in (
            self.path_entry,
            self.file_button,
            self.folder_button,
            self.scan_button,
            self.cache_check,
            self.hidden_check,
            self.max_files_spin,
            self.key_button,
            self.verify_button,
        ):
            widget.configure(state=idle_state)
        self.cancel_button.configure(
            state=tk.NORMAL if busy and cancellable else tk.DISABLED
        )
        if busy:
            self._set_export_state(tk.DISABLED)
        else:
            self._set_export_state(
                tk.NORMAL if self._last_records else tk.DISABLED
            )

    def _set_export_state(self, state: str) -> None:
        for button in (
            self.jsonl_button,
            self.report_json_button,
            self.report_html_button,
            self.case_button,
        ):
            button.configure(state=state)

    def _clear_results(self) -> None:
        for item in self.tree.get_children():
            self.tree.delete(item)
        self._set_details("")

    def _refresh_table(self) -> None:
        if not hasattr(self, "tree"):
            return
        selected_path = None
        selection = self.tree.selection()
        if selection:
            selected = self._visible_records.get(selection[0])
            selected_path = selected.get("path") if selected else None

        self._clear_results()
        self._visible_records = {}
        visible = filter_records(
            self._last_records,
            self.search_var.get(),
            self.status_filter_var.get(),
        )
        selected_item = None
        for index, record in enumerate(visible):
            item = f"record-{index}"
            self._visible_records[item] = record
            self.tree.insert(
                "",
                tk.END,
                iid=item,
                values=(
                    record.get("path", ""),
                    record.get("mime", ""),
                    format_bytes(int(record.get("size_bytes") or 0)),
                    record_status(record),
                ),
            )
            if record.get("path") == selected_path:
                selected_item = item

        if selected_item is not None:
            self.tree.selection_set(selected_item)
            self.tree.see(selected_item)
        elif visible:
            first = self.tree.get_children()[0]
            self.tree.selection_set(first)
            self._show_selected_record()

        if self._last_records:
            self.status_var.set(
                f"Showing {len(visible)} of {len(self._last_records)} file(s)."
            )

    def _show_selected_record(self, _event: Any = None) -> None:
        selection = self.tree.selection()
        if not selection:
            self._set_details("")
            return
        record = self._visible_records.get(selection[0])
        if record is None:
            self._set_details("")
            return
        self._set_details(
            json.dumps(record, ensure_ascii=False, sort_keys=True, indent=2)
        )

    def _set_details(self, text: str) -> None:
        self.details.configure(state=tk.NORMAL)
        self.details.delete("1.0", tk.END)
        if text:
            self.details.insert("1.0", text + "\n")
        self.details.configure(state=tk.DISABLED)

    def _export_jsonl(self) -> None:
        dialogs.export_jsonl(self, self._last_records)

    def _export_report(self, format_name: str) -> None:
        dialogs.export_report(self, self._last_records, format_name)

    def _export_case(self) -> None:
        dialogs.export_case(self, self._last_records, self._scan_base)

    def _generate_keypair(self) -> None:
        dialogs.generate_keypair(self)

    def _verify_case(self) -> None:
        request = dialogs.choose_verification(self)
        if request is None:
            return

        self.progress.configure(mode="indeterminate")
        self.progress.start(12)
        self.status_var.set(f"Verifying: {Path(request.bundle).name}")
        self._set_busy(True)
        self._worker = threading.Thread(
            target=run_verification,
            args=(
                self._events,
                request.bundle,
                request.files_base,
                request.public_key,
            ),
            name="metaxtract-verify",
            daemon=True,
        )
        self._worker.start()

    def _finish_verification(
        self,
        bundle: str | None,
        issues: list[dict[str, str]] | None,
    ) -> None:
        self.progress.stop()
        self.progress.configure(mode="determinate", maximum=1, value=0)
        self._worker = None
        self._set_busy(False)
        if bundle is None or issues is None:
            self.status_var.set("Verification failed.")
            return
        if issues:
            self.status_var.set(f"Verification found {len(issues)} issue(s).")
            self._show_verification_issues(bundle, issues)
            return
        self.progress.configure(value=1)
        self.status_var.set(f"Verified: {Path(bundle).name}")
        messagebox.showinfo("MetaXtract", "Bundle verification passed.")

    def _show_verification_issues(
        self,
        bundle: str,
        issues: list[dict[str, str]],
    ) -> None:
        dialogs.show_verification_issues(self, bundle, issues)

    def _close(self) -> None:
        if self._cancel_event is not None:
            self._cancel_event.set()
        self.destroy()


def main() -> None:
    app = MetaXtractGUI()
    app.mainloop()


if __name__ == "__main__":
    main()
