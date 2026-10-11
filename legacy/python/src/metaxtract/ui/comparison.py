from __future__ import annotations

import json
import tkinter as tk
from dataclasses import asdict
from pathlib import Path
from tkinter import ttk
from typing import Any

from metaxtract.core.workspace import CaseWorkspace, ScanSummary
from metaxtract.reporting.diff import DiffSummary, diff_records
from metaxtract.ui.icons import apply_window_icons


def _scan_label(summary: ScanSummary) -> str:
    timestamp = summary.started_at.replace("T", " ").replace("+00:00", " UTC")
    source = Path(summary.source_path).name or summary.source_path
    return f"{timestamp} · {summary.status} · {summary.record_count} file(s) · {source}"


def _short_value(value: Any, present: bool) -> str:
    if not present:
        return "<missing>"
    rendered = json.dumps(value, ensure_ascii=False, sort_keys=True)
    return rendered if len(rendered) <= 120 else f"{rendered[:117]}…"


class ScanComparisonDialog(tk.Toplevel):
    def __init__(
        self,
        parent: tk.Misc,
        workspace: CaseWorkspace,
        scans: list[ScanSummary],
        current_scan_id: str | None,
    ) -> None:
        super().__init__(parent)
        self.title("Compare Scans")
        self.geometry("980x650")
        self.minsize(760, 480)
        self.transient(parent)
        self._window_icons = apply_window_icons(self)
        self._workspace = workspace
        self._scans = scans
        self._rows: dict[str, dict[str, Any]] = {}

        self.before_var = tk.StringVar(self)
        self.after_var = tk.StringVar(self)
        self.summary_var = tk.StringVar(self, value="Choose two scans to compare.")

        self._build()
        self._select_defaults(current_scan_id)
        self._compare()

    def _build(self) -> None:
        self.columnconfigure(0, weight=1)
        self.rowconfigure(2, weight=1)

        controls = ttk.Frame(self, padding=10)
        controls.grid(row=0, column=0, sticky=tk.EW)
        controls.columnconfigure(1, weight=1)
        controls.columnconfigure(3, weight=1)
        labels = [_scan_label(summary) for summary in self._scans]

        ttk.Label(controls, text="Earlier").grid(row=0, column=0, sticky=tk.W)
        self.before_combo = ttk.Combobox(
            controls,
            textvariable=self.before_var,
            values=labels,
            state="readonly",
        )
        self.before_combo.grid(row=0, column=1, sticky=tk.EW, padx=(6, 12))
        ttk.Label(controls, text="Later").grid(row=0, column=2, sticky=tk.W)
        self.after_combo = ttk.Combobox(
            controls,
            textvariable=self.after_var,
            values=labels,
            state="readonly",
        )
        self.after_combo.grid(row=0, column=3, sticky=tk.EW, padx=(6, 12))
        ttk.Button(controls, text="Compare", command=self._compare).grid(
            row=0,
            column=4,
        )
        self.before_combo.bind("<<ComboboxSelected>>", lambda _event: self._compare())
        self.after_combo.bind("<<ComboboxSelected>>", lambda _event: self._compare())

        ttk.Label(self, textvariable=self.summary_var, padding=(10, 0, 10, 8)).grid(
            row=1,
            column=0,
            sticky=tk.W,
        )

        pane = ttk.Panedwindow(self, orient=tk.VERTICAL)
        pane.grid(row=2, column=0, sticky=tk.NSEW, padx=10, pady=(0, 10))
        table_frame = ttk.Frame(pane)
        detail_frame = ttk.Frame(pane)
        pane.add(table_frame, weight=3)
        pane.add(detail_frame, weight=2)

        columns = ("change", "field", "before", "after")
        self.tree = ttk.Treeview(table_frame, columns=columns, show="tree headings")
        self.tree.heading("#0", text="Path")
        self.tree.heading("change", text="Change")
        self.tree.heading("field", text="Field")
        self.tree.heading("before", text="Before")
        self.tree.heading("after", text="After")
        self.tree.column("#0", width=260, minwidth=150)
        self.tree.column("change", width=85, minwidth=70, anchor=tk.CENTER)
        self.tree.column("field", width=210, minwidth=120)
        self.tree.column("before", width=180, minwidth=100)
        self.tree.column("after", width=180, minwidth=100)
        scroll = ttk.Scrollbar(table_frame, orient=tk.VERTICAL, command=self.tree.yview)
        self.tree.configure(yscrollcommand=scroll.set)
        self.tree.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
        scroll.pack(side=tk.RIGHT, fill=tk.Y)
        self.tree.tag_configure("added", foreground="#087f23")
        self.tree.tag_configure("removed", foreground="#b42318")
        self.tree.tag_configure("changed", foreground="#9a6700")
        self.tree.bind("<<TreeviewSelect>>", self._show_details)

        ttk.Label(detail_frame, text="Selected change").pack(anchor=tk.W, pady=(8, 4))
        self.details = tk.Text(detail_frame, wrap="word", height=9, state=tk.DISABLED)
        self.details.pack(fill=tk.BOTH, expand=True)

        actions = ttk.Frame(self, padding=(10, 0, 10, 10))
        actions.grid(row=3, column=0, sticky=tk.E)
        ttk.Button(actions, text="Close", command=self.destroy).pack()
        self.bind("<Escape>", lambda _event: self.destroy())

    def _select_defaults(self, current_scan_id: str | None) -> None:
        later_index = next(
            (
                index
                for index, summary in enumerate(self._scans)
                if summary.scan_id == current_scan_id
            ),
            0,
        )
        if later_index + 1 < len(self._scans):
            earlier_index = later_index + 1
        else:
            earlier_index = later_index
            later_index -= 1
        self.before_combo.current(earlier_index)
        self.after_combo.current(later_index)

    def _compare(self) -> None:
        before_index = self.before_combo.current()
        after_index = self.after_combo.current()
        if before_index < 0 or after_index < 0 or before_index == after_index:
            self.summary_var.set("Choose two different scans.")
            return

        before_scan = self._scans[before_index]
        after_scan = self._scans[after_index]
        before_records = self._workspace.load_records(before_scan.scan_id)
        after_records = self._workspace.load_records(after_scan.scan_id)
        result = diff_records(before_records, after_records)
        unchanged = len(
            set(record["path"] for record in before_records)
            & set(record["path"] for record in after_records)
        ) - len(result.changed)
        self.summary_var.set(
            f"Added {len(result.added)} · Removed {len(result.removed)} · "
            f"Changed {len(result.changed)} · Unchanged {max(unchanged, 0)}"
        )
        self._populate(result, before_records, after_records)

    def _populate(
        self,
        result: DiffSummary,
        before_records: list[dict[str, Any]],
        after_records: list[dict[str, Any]],
    ) -> None:
        for item in self.tree.get_children():
            self.tree.delete(item)
        self._rows = {}
        before_index = {record["path"]: record for record in before_records}
        after_index = {record["path"]: record for record in after_records}

        for path in result.added:
            item = self.tree.insert(
                "", tk.END, text=path, values=("Added", "", "", ""), tags=("added",)
            )
            self._rows[item] = {"change": "added", "path": path, "after": after_index[path]}
        for path in result.removed:
            item = self.tree.insert(
                "", tk.END, text=path, values=("Removed", "", "", ""), tags=("removed",)
            )
            self._rows[item] = {"change": "removed", "path": path, "before": before_index[path]}
        for path in result.changed:
            parent = self.tree.insert(
                "", tk.END, text=path, values=("Changed", "", "", ""), tags=("changed",), open=True
            )
            self._rows[parent] = {
                "change": "changed",
                "path": path,
                "before": before_index[path],
                "after": after_index[path],
            }
            for change in result.field_changes[path]:
                child = self.tree.insert(
                    parent,
                    tk.END,
                    values=(
                        "",
                        change.field,
                        _short_value(change.before, change.before_present),
                        _short_value(change.after, change.after_present),
                    ),
                    tags=("changed",),
                )
                self._rows[child] = {
                    "change": "field",
                    "path": path,
                    **asdict(change),
                }

        children = self.tree.get_children()
        if children:
            self.tree.selection_set(children[0])
            self.tree.see(children[0])
            self._show_details()
        else:
            self._set_details("No differences.")

    def _show_details(self, _event: Any = None) -> None:
        selection = self.tree.selection()
        row = self._rows.get(selection[0]) if selection else None
        self._set_details(
            json.dumps(row, ensure_ascii=False, sort_keys=True, indent=2)
            if row is not None
            else ""
        )

    def _set_details(self, text: str) -> None:
        self.details.configure(state=tk.NORMAL)
        self.details.delete("1.0", tk.END)
        if text:
            self.details.insert("1.0", f"{text}\n")
        self.details.configure(state=tk.DISABLED)


def show_scan_comparison(
    parent: tk.Misc,
    workspace: CaseWorkspace,
    scans: list[ScanSummary],
    current_scan_id: str | None,
) -> None:
    dialog = ScanComparisonDialog(parent, workspace, scans, current_scan_id)
    dialog.grab_set()
    dialog.focus_set()
    parent.wait_window(dialog)
