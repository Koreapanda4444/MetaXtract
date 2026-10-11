from __future__ import annotations

import tkinter as tk
from collections import Counter
from dataclasses import dataclass
from pathlib import Path
from tkinter import ttk

from metaxtract.core.workspace import CaseInfo, CaseWorkspace, ScanSummary
from metaxtract.reporting.diff import diff_records
from metaxtract.ui.icons import apply_window_icons
from metaxtract.ui.records import format_bytes, record_status


@dataclass(frozen=True)
class MimeSummary:
    mime: str
    count: int
    size_bytes: int


@dataclass(frozen=True)
class ChangeSummary:
    earlier: ScanSummary
    added: int
    removed: int
    changed: int
    unchanged: int


@dataclass(frozen=True)
class DashboardSnapshot:
    case: CaseInfo
    scans: tuple[ScanSummary, ...]
    selected_scan: ScanSummary | None
    source_count: int
    stored_record_count: int
    warning_count: int
    error_count: int
    annotation_count: int
    status_counts: tuple[tuple[str, int], ...]
    origin_counts: tuple[tuple[str, int], ...]
    tag_counts: tuple[tuple[str, int], ...]
    selected_size_bytes: int
    selected_health: tuple[tuple[str, int], ...]
    mime_summaries: tuple[MimeSummary, ...]
    changes: ChangeSummary | None


def _counter_items(counter: Counter[str]) -> tuple[tuple[str, int], ...]:
    return tuple(sorted(counter.items(), key=lambda item: (-item[1], item[0].casefold())))


def build_dashboard_snapshot(
    workspace: CaseWorkspace,
    selected_scan_id: str | None = None,
) -> DashboardSnapshot:
    case = workspace.info()
    scans = workspace.list_scans()
    selected_index = next(
        (index for index, scan in enumerate(scans) if scan.scan_id == selected_scan_id),
        0,
    )
    selected_scan = scans[selected_index] if scans else None
    selected_records = (
        workspace.load_records(selected_scan.scan_id) if selected_scan is not None else []
    )

    annotations = []
    for scan in scans:
        annotations.extend(workspace.list_annotations(scan.scan_id).values())
    tags = Counter(tag for annotation in annotations for tag in annotation.tags)

    health = Counter(record_status(record) for record in selected_records)
    mime_counts: Counter[str] = Counter()
    mime_sizes: Counter[str] = Counter()
    for record in selected_records:
        mime = str(record.get("mime") or "application/octet-stream")
        mime_counts[mime] += 1
        mime_sizes[mime] += int(record.get("size_bytes") or 0)
    mime_summaries = tuple(
        MimeSummary(mime, count, mime_sizes[mime]) for mime, count in _counter_items(mime_counts)
    )

    changes = None
    if selected_scan is not None and selected_index + 1 < len(scans):
        earlier = scans[selected_index + 1]
        earlier_records = workspace.load_records(earlier.scan_id)
        comparison = diff_records(earlier_records, selected_records)
        common_paths = {str(record["path"]) for record in earlier_records} & {
            str(record["path"]) for record in selected_records
        }
        changes = ChangeSummary(
            earlier=earlier,
            added=len(comparison.added),
            removed=len(comparison.removed),
            changed=len(comparison.changed),
            unchanged=max(len(common_paths) - len(comparison.changed), 0),
        )

    return DashboardSnapshot(
        case=case,
        scans=tuple(scans),
        selected_scan=selected_scan,
        source_count=len({scan.source_path.casefold() for scan in scans}),
        stored_record_count=sum(scan.record_count for scan in scans),
        warning_count=sum(scan.warning_count for scan in scans),
        error_count=sum(scan.error_count for scan in scans),
        annotation_count=len(annotations),
        status_counts=_counter_items(Counter(scan.status for scan in scans)),
        origin_counts=_counter_items(Counter(scan.origin for scan in scans)),
        tag_counts=_counter_items(tags),
        selected_size_bytes=sum(int(record.get("size_bytes") or 0) for record in selected_records),
        selected_health=tuple(
            (status, health.get(status, 0)) for status in ("OK", "Warning", "Error")
        ),
        mime_summaries=mime_summaries,
        changes=changes,
    )


def _format_timestamp(value: str | None) -> str:
    if not value:
        return "—"
    return value.replace("T", " ").replace("+00:00", " UTC")


def _counts_text(values: tuple[tuple[str, int], ...]) -> str:
    return " · ".join(f"{name}: {count}" for name, count in values) or "None"


class CaseDashboardDialog(tk.Toplevel):
    def __init__(self, parent: tk.Misc, snapshot: DashboardSnapshot) -> None:
        super().__init__(parent)
        self.title("Case Dashboard")
        self.geometry("920x680")
        self.minsize(720, 520)
        self.transient(parent)
        self._window_icons = apply_window_icons(self)
        self._snapshot = snapshot
        self._build()

    def _build(self) -> None:
        self.columnconfigure(0, weight=1)
        self.rowconfigure(4, weight=1)

        header = ttk.Frame(self, padding=(14, 12, 14, 6))
        header.grid(row=0, column=0, sticky=tk.EW)
        header.columnconfigure(0, weight=1)
        ttk.Label(
            header,
            text=self._snapshot.case.title,
            font=("TkDefaultFont", 16, "bold"),
        ).grid(row=0, column=0, sticky=tk.W)
        ttk.Label(
            header,
            text=f"Case ID: {self._snapshot.case.case_id}",
        ).grid(row=1, column=0, sticky=tk.W, pady=(3, 0))
        ttk.Label(
            header,
            text=(
                f"Created {_format_timestamp(self._snapshot.case.created_at)} · "
                f"Updated {_format_timestamp(self._snapshot.case.updated_at)}"
            ),
        ).grid(row=2, column=0, sticky=tk.W, pady=(2, 0))
        if self._snapshot.case.notes:
            ttk.Label(
                header,
                text=self._snapshot.case.notes,
                wraplength=840,
                justify=tk.LEFT,
            ).grid(row=3, column=0, sticky=tk.EW, pady=(8, 0))

        cards = ttk.Frame(self, padding=(14, 4))
        cards.grid(row=1, column=0, sticky=tk.EW)
        values = (
            ("Scans", len(self._snapshot.scans)),
            ("Sources", self._snapshot.source_count),
            ("Stored records", self._snapshot.stored_record_count),
            ("Warnings", self._snapshot.warning_count),
            ("Errors", self._snapshot.error_count),
            ("Annotations", self._snapshot.annotation_count),
        )
        for column, (label, value) in enumerate(values):
            cards.columnconfigure(column, weight=1)
            card = ttk.LabelFrame(cards, text=label, padding=(10, 5))
            card.grid(
                row=0,
                column=column,
                sticky=tk.EW,
                padx=(0 if column == 0 else 4, 0),
            )
            ttk.Label(
                card,
                text=f"{value:,}",
                font=("TkDefaultFont", 13, "bold"),
            ).pack()

        selected = ttk.LabelFrame(self, text="Selected scan", padding=10)
        selected.grid(row=2, column=0, sticky=tk.EW, padx=14, pady=(8, 0))
        selected.columnconfigure(0, weight=1)
        scan = self._snapshot.selected_scan
        if scan is None:
            ttk.Label(selected, text="No scans have been saved yet.").grid(
                row=0,
                column=0,
                sticky=tk.W,
            )
        else:
            source = Path(scan.source_path).name or scan.source_path
            ttk.Label(
                selected,
                text=(
                    f"{_format_timestamp(scan.started_at)} · {scan.status} · "
                    f"{scan.origin} · {source}"
                ),
            ).grid(row=0, column=0, sticky=tk.W)
            ttk.Label(
                selected,
                text=(
                    f"{scan.record_count:,} file(s), "
                    f"{format_bytes(self._snapshot.selected_size_bytes)} · "
                    f"{_counts_text(self._snapshot.selected_health)}"
                ),
            ).grid(row=1, column=0, sticky=tk.W, pady=(4, 0))

        comparison = ttk.LabelFrame(self, text="Change from previous scan", padding=10)
        comparison.grid(row=3, column=0, sticky=tk.EW, padx=14, pady=(8, 0))
        changes = self._snapshot.changes
        if changes is None:
            text = "No earlier scan is available for this selection."
        else:
            text = (
                f"Added {changes.added:,} · Removed {changes.removed:,} · "
                f"Changed {changes.changed:,} · Unchanged {changes.unchanged:,}  "
                f"(since {_format_timestamp(changes.earlier.started_at)})"
            )
        ttk.Label(comparison, text=text).pack(anchor=tk.W)

        notebook = ttk.Notebook(self)
        notebook.grid(row=4, column=0, sticky=tk.NSEW, padx=14, pady=(8, 8))
        notebook.add(self._build_mime_tab(notebook), text="MIME types")
        notebook.add(self._build_tag_tab(notebook), text="Tags")
        notebook.add(self._build_scan_tab(notebook), text="Scan history")

        footer = ttk.Frame(self, padding=(14, 0, 14, 12))
        footer.grid(row=5, column=0, sticky=tk.EW)
        ttk.Label(
            footer,
            text=(
                f"Scan status — {_counts_text(self._snapshot.status_counts)}    "
                f"Origins — {_counts_text(self._snapshot.origin_counts)}"
            ),
        ).pack(side=tk.LEFT)
        ttk.Button(footer, text="Close", command=self.destroy).pack(side=tk.RIGHT)
        self.bind("<Escape>", lambda _event: self.destroy())

    @staticmethod
    def _tree_tab(
        parent: ttk.Notebook,
        columns: tuple[str, ...],
        headings: tuple[str, ...],
        widths: tuple[int, ...],
    ) -> tuple[ttk.Frame, ttk.Treeview]:
        frame = ttk.Frame(parent, padding=6)
        tree = ttk.Treeview(frame, columns=columns, show="headings")
        for column, heading, width in zip(columns, headings, widths, strict=True):
            tree.heading(column, text=heading)
            tree.column(column, width=width, minwidth=70)
        scroll = ttk.Scrollbar(frame, orient=tk.VERTICAL, command=tree.yview)
        tree.configure(yscrollcommand=scroll.set)
        tree.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
        scroll.pack(side=tk.RIGHT, fill=tk.Y)
        return frame, tree

    def _build_mime_tab(self, parent: ttk.Notebook) -> ttk.Frame:
        frame, tree = self._tree_tab(
            parent,
            ("mime", "count", "size"),
            ("MIME type", "Files", "Size"),
            (480, 100, 140),
        )
        tree.column("count", anchor=tk.E)
        tree.column("size", anchor=tk.E)
        for summary in self._snapshot.mime_summaries:
            tree.insert(
                "",
                tk.END,
                values=(summary.mime, f"{summary.count:,}", format_bytes(summary.size_bytes)),
            )
        return frame

    def _build_tag_tab(self, parent: ttk.Notebook) -> ttk.Frame:
        frame, tree = self._tree_tab(
            parent,
            ("tag", "count"),
            ("Tag", "Uses"),
            (560, 120),
        )
        tree.column("count", anchor=tk.E)
        for tag, count in self._snapshot.tag_counts:
            tree.insert("", tk.END, values=(tag, f"{count:,}"))
        return frame

    def _build_scan_tab(self, parent: ttk.Notebook) -> ttk.Frame:
        frame, tree = self._tree_tab(
            parent,
            ("started", "status", "origin", "records", "issues", "source"),
            ("Started", "Status", "Origin", "Files", "W / E", "Source"),
            (170, 90, 110, 80, 80, 300),
        )
        tree.column("records", anchor=tk.E)
        tree.column("issues", anchor=tk.CENTER)
        for scan in self._snapshot.scans:
            tree.insert(
                "",
                tk.END,
                values=(
                    _format_timestamp(scan.started_at),
                    scan.status,
                    scan.origin,
                    f"{scan.record_count:,}",
                    f"{scan.warning_count:,} / {scan.error_count:,}",
                    scan.source_path,
                ),
            )
        return frame


def show_case_dashboard(
    parent: tk.Misc,
    workspace: CaseWorkspace,
    selected_scan_id: str | None,
) -> None:
    snapshot = build_dashboard_snapshot(workspace, selected_scan_id)
    dialog = CaseDashboardDialog(parent, snapshot)
    dialog.grab_set()
    dialog.focus_set()
    parent.wait_window(dialog)
