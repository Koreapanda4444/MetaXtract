from __future__ import annotations

import json
import os
import queue
import stat
import tempfile
import threading
import tkinter as tk
from dataclasses import dataclass
from pathlib import Path
from tkinter import filedialog, messagebox, ttk
from typing import Any

from metaxtract.core.jsonio import dumps_json
from metaxtract.reporting.duplicates import (
    ExactDuplicateGroup,
    VisualMatch,
    VisualSkip,
    exact_duplicate_groups,
    find_visual_matches,
)
from metaxtract.reporting.findings import Finding, classify_findings
from metaxtract.reporting.geo import (
    GeoLocation,
    extract_locations,
    geojson_document,
    render_kml,
)
from metaxtract.reporting.timeline import TimelineEvent, build_timeline
from metaxtract.ui.icons import apply_window_icons
from metaxtract.ui.records import format_bytes


@dataclass(frozen=True)
class AnalysisSnapshot:
    timeline: tuple[TimelineEvent, ...]
    locations: tuple[GeoLocation, ...]
    exact_duplicates: tuple[ExactDuplicateGroup, ...]
    findings: tuple[Finding, ...]


def build_analysis_snapshot(records: list[dict[str, Any]]) -> AnalysisSnapshot:
    return AnalysisSnapshot(
        timeline=tuple(build_timeline(records)),
        locations=tuple(extract_locations(records)),
        exact_duplicates=tuple(exact_duplicate_groups(records)),
        findings=tuple(classify_findings(records)),
    )


def _short_value(value: Any, limit: int = 180) -> str:
    rendered = json.dumps(value, ensure_ascii=False, sort_keys=True)
    return rendered if len(rendered) <= limit else f"{rendered[: limit - 1]}…"


def _atomic_write_text(path: str | Path, text: str) -> None:
    output = Path(path)
    output.parent.mkdir(parents=True, exist_ok=True)
    output_mode = stat.S_IMODE(output.stat().st_mode) if output.exists() else 0o644
    handle = tempfile.NamedTemporaryFile(
        mode="w",
        encoding="utf-8",
        newline="\n",
        prefix=f".{output.name}.",
        suffix=".tmp",
        dir=output.parent,
        delete=False,
    )
    temporary = Path(handle.name)
    try:
        with handle as stream:
            stream.write(text)
            stream.flush()
            os.fsync(stream.fileno())
        temporary.chmod(output_mode)
        temporary.replace(output)
    except Exception:
        handle.close()
        temporary.unlink(missing_ok=True)
        raise


def _tree_frame(
    parent: tk.Misc,
    columns: tuple[str, ...],
    headings: tuple[str, ...],
    widths: tuple[int, ...],
    *,
    tree_heading: str | None = None,
) -> tuple[ttk.Frame, ttk.Treeview]:
    frame = ttk.Frame(parent)
    frame.columnconfigure(0, weight=1)
    frame.rowconfigure(0, weight=1)
    show = "tree headings" if tree_heading is not None else "headings"
    tree = ttk.Treeview(frame, columns=columns, show=show)
    if tree_heading is not None:
        tree.heading("#0", text=tree_heading)
        tree.column("#0", width=260, minwidth=120)
    for column, heading, width in zip(columns, headings, widths, strict=True):
        tree.heading(column, text=heading)
        tree.column(column, width=width, minwidth=70)
    vertical = ttk.Scrollbar(frame, orient=tk.VERTICAL, command=tree.yview)
    horizontal = ttk.Scrollbar(frame, orient=tk.HORIZONTAL, command=tree.xview)
    tree.configure(yscrollcommand=vertical.set, xscrollcommand=horizontal.set)
    tree.grid(row=0, column=0, sticky=tk.NSEW)
    vertical.grid(row=0, column=1, sticky=tk.NS)
    horizontal.grid(row=1, column=0, sticky=tk.EW)
    return frame, tree


class AnalysisDialog(tk.Toplevel):
    def __init__(
        self,
        parent: tk.Misc,
        records: list[dict[str, Any]],
        files_base: Path | None,
    ) -> None:
        prepared_records = list(records)
        snapshot = build_analysis_snapshot(prepared_records)
        super().__init__(parent)
        self.title("Metadata Analysis")
        self.geometry("1120x720")
        self.minsize(820, 540)
        self.transient(parent)
        self._window_icons = apply_window_icons(self)
        self._records = prepared_records
        self._files_base = files_base
        self._snapshot = snapshot
        self._finding_rows: dict[str, Finding] = {}
        self._visual_events: queue.Queue[tuple[str, Any]] = queue.Queue()
        self._visual_worker: threading.Thread | None = None
        self._visual_after_id: str | None = None
        self._closed = False
        self._build()

    def _build(self) -> None:
        self.columnconfigure(0, weight=1)
        self.rowconfigure(1, weight=1)

        duplicate_files = sum(item.file_count for item in self._snapshot.exact_duplicates)
        summary = (
            f"{len(self._records):,} files  ·  "
            f"{len(self._snapshot.timeline):,} timeline events  ·  "
            f"{len(self._snapshot.locations):,} locations  ·  "
            f"{len(self._snapshot.exact_duplicates):,} exact groups "
            f"({duplicate_files:,} files)  ·  {len(self._snapshot.findings):,} findings"
        )
        ttk.Label(self, text=summary, padding=(12, 10, 12, 4)).grid(
            row=0,
            column=0,
            sticky=tk.W,
        )

        notebook = ttk.Notebook(self)
        notebook.grid(row=1, column=0, sticky=tk.NSEW, padx=12, pady=(4, 8))
        notebook.add(self._timeline_tab(notebook), text="Timeline")
        notebook.add(self._locations_tab(notebook), text="Locations")
        notebook.add(self._duplicates_tab(notebook), text="Duplicates")
        notebook.add(self._findings_tab(notebook), text="Findings")

        footer = ttk.Frame(self, padding=(12, 0, 12, 10))
        footer.grid(row=2, column=0, sticky=tk.E)
        ttk.Button(footer, text="Close", command=self._close).pack()
        self.bind("<Escape>", lambda _event: self._close())
        self.protocol("WM_DELETE_WINDOW", self._close)

    def _timeline_tab(self, parent: ttk.Notebook) -> ttk.Frame:
        frame, tree = _tree_frame(
            parent,
            ("timestamp", "event", "path", "source", "value", "timezone", "precision"),
            ("Timestamp", "Event", "Path", "Source field", "Original value", "TZ", "Precision"),
            (190, 85, 230, 210, 220, 70, 80),
        )
        for event in self._snapshot.timeline:
            tree.insert(
                "",
                tk.END,
                values=(
                    event.timestamp,
                    event.event,
                    event.path,
                    event.source_field,
                    _short_value(event.source_value),
                    "known" if event.timezone_known else "unknown",
                    event.precision,
                ),
            )
        return frame

    def _locations_tab(self, parent: ttk.Notebook) -> ttk.Frame:
        frame = ttk.Frame(parent, padding=6)
        frame.columnconfigure(0, weight=1)
        frame.rowconfigure(1, weight=1)

        actions = ttk.Frame(frame)
        actions.grid(row=0, column=0, sticky=tk.EW, pady=(0, 6))
        ttk.Label(
            actions,
            text="Offline coordinates only; no map tiles or external APIs are contacted.",
        ).pack(side=tk.LEFT)
        export_state = tk.NORMAL if self._snapshot.locations else tk.DISABLED
        ttk.Button(
            actions,
            text="Export GeoJSON…",
            command=lambda: self._export_locations("geojson"),
            state=export_state,
        ).pack(side=tk.RIGHT, padx=(6, 0))
        ttk.Button(
            actions,
            text="Export KML…",
            command=lambda: self._export_locations("kml"),
            state=export_state,
        ).pack(side=tk.RIGHT)

        table, tree = _tree_frame(
            frame,
            ("path", "latitude", "longitude", "altitude", "source"),
            ("Path", "Latitude", "Longitude", "Altitude (m)", "Source fields"),
            (300, 120, 120, 100, 360),
        )
        table.grid(row=1, column=0, sticky=tk.NSEW)
        for location in self._snapshot.locations:
            source_fields = f"{location.latitude_field}; {location.longitude_field}"
            if location.altitude_field:
                source_fields += f"; {location.altitude_field}"
            tree.insert(
                "",
                tk.END,
                values=(
                    location.path,
                    format(location.latitude, ".10g"),
                    format(location.longitude, ".10g"),
                    "" if location.altitude_m is None else format(location.altitude_m, ".10g"),
                    source_fields,
                ),
            )
        return frame

    def _duplicates_tab(self, parent: ttk.Notebook) -> ttk.Frame:
        frame = ttk.Frame(parent, padding=6)
        frame.columnconfigure(0, weight=1)
        frame.rowconfigure(0, weight=1)
        frame.rowconfigure(1, weight=1)

        exact = ttk.LabelFrame(frame, text="Exact SHA-256 duplicates", padding=6)
        exact.grid(row=0, column=0, sticky=tk.NSEW, pady=(0, 6))
        exact.columnconfigure(0, weight=1)
        exact.rowconfigure(0, weight=1)
        exact_table, exact_tree = _tree_frame(
            exact,
            ("files", "size", "reclaimable"),
            ("Files", "Each size", "Potential reclaim"),
            (80, 120, 130),
            tree_heading="SHA-256 / path",
        )
        exact_table.grid(row=0, column=0, sticky=tk.NSEW)
        for group in self._snapshot.exact_duplicates:
            parent_id = exact_tree.insert(
                "",
                tk.END,
                text=group.sha256,
                values=(
                    group.file_count,
                    "—" if group.size_bytes is None else format_bytes(group.size_bytes),
                    "—"
                    if group.reclaimable_bytes is None
                    else format_bytes(group.reclaimable_bytes),
                ),
                open=True,
            )
            for path in group.paths:
                exact_tree.insert(parent_id, tk.END, text=path, values=("", "", ""))

        visual = ttk.LabelFrame(frame, text="Visual image similarity", padding=6)
        visual.grid(row=1, column=0, sticky=tk.NSEW)
        visual.columnconfigure(0, weight=1)
        visual.rowconfigure(1, weight=1)
        controls = ttk.Frame(visual)
        controls.grid(row=0, column=0, sticky=tk.EW, pady=(0, 6))
        self.visual_status_var = tk.StringVar(
            self,
            value=(
                "Ready to compare integrity-checked local images."
                if self._files_base is not None
                else "Original folder unavailable; exact duplicate results remain available."
            ),
        )
        ttk.Label(controls, textvariable=self.visual_status_var).pack(
            side=tk.LEFT, fill=tk.X, expand=True
        )
        ttk.Label(controls, text="Max distance").pack(side=tk.LEFT, padx=(8, 4))
        self.visual_threshold_var = tk.StringVar(self, value="8")
        self.visual_threshold = ttk.Spinbox(
            controls,
            from_=0,
            to=64,
            width=5,
            textvariable=self.visual_threshold_var,
            state="normal" if self._files_base is not None else "disabled",
        )
        self.visual_threshold.pack(side=tk.LEFT)
        self.visual_button = ttk.Button(
            controls,
            text="Analyze originals",
            command=self._start_visual_analysis,
            state=tk.NORMAL if self._files_base is not None else tk.DISABLED,
        )
        self.visual_button.pack(side=tk.LEFT, padx=(6, 0))
        self.visual_progress = ttk.Progressbar(controls, mode="indeterminate", length=90)
        self.visual_progress.pack(side=tk.LEFT, padx=(6, 0))

        visual_table, self.visual_tree = _tree_frame(
            visual,
            ("status", "first", "second", "distance", "hashes"),
            ("Status", "First path", "Second path / reason", "Distance", "Perceptual hashes"),
            (80, 270, 270, 80, 270),
        )
        visual_table.grid(row=1, column=0, sticky=tk.NSEW)
        return frame

    def _findings_tab(self, parent: ttk.Notebook) -> ttk.Frame:
        frame = ttk.Frame(parent, padding=6)
        frame.columnconfigure(0, weight=1)
        frame.rowconfigure(0, weight=3)
        frame.rowconfigure(1, weight=2)
        table, tree = _tree_frame(
            frame,
            ("severity", "category", "rule", "path", "title"),
            ("Severity", "Category", "Rule", "Path", "Finding"),
            (85, 100, 220, 310, 260),
        )
        table.grid(row=0, column=0, sticky=tk.NSEW)
        tree.tag_configure("high", foreground="#b42318")
        tree.tag_configure("medium", foreground="#9a6700")
        tree.tag_configure("low", foreground="#475467")
        for index, finding in enumerate(self._snapshot.findings):
            item = f"finding-{index}"
            tree.insert(
                "",
                tk.END,
                iid=item,
                values=(
                    finding.severity,
                    finding.category,
                    finding.rule_id,
                    finding.path,
                    finding.title,
                ),
                tags=(finding.severity,),
            )
            self._finding_rows[item] = finding

        details_frame = ttk.LabelFrame(frame, text="Reason and source evidence", padding=6)
        details_frame.grid(row=1, column=0, sticky=tk.NSEW, pady=(6, 0))
        details_frame.columnconfigure(0, weight=1)
        details_frame.rowconfigure(0, weight=1)
        self.finding_details = tk.Text(details_frame, wrap="word", state=tk.DISABLED)
        detail_scroll = ttk.Scrollbar(
            details_frame,
            orient=tk.VERTICAL,
            command=self.finding_details.yview,
        )
        self.finding_details.configure(yscrollcommand=detail_scroll.set)
        self.finding_details.grid(row=0, column=0, sticky=tk.NSEW)
        detail_scroll.grid(row=0, column=1, sticky=tk.NS)
        tree.bind("<<TreeviewSelect>>", lambda _event: self._show_finding(tree))
        if self._snapshot.findings:
            tree.selection_set("finding-0")
            self._show_finding(tree)
        return frame

    def _show_finding(self, tree: ttk.Treeview) -> None:
        selection = tree.selection()
        finding = self._finding_rows.get(selection[0]) if selection else None
        if finding is None:
            text = ""
        else:
            evidence = "\n".join(
                f"• {item.field}: {_short_value(item.value, 500)}" for item in finding.evidence
            )
            text = f"{finding.detail}\n\n{evidence}"
        self.finding_details.configure(state=tk.NORMAL)
        self.finding_details.delete("1.0", tk.END)
        if text:
            self.finding_details.insert("1.0", text)
        self.finding_details.configure(state=tk.DISABLED)

    def _export_locations(self, format_name: str) -> None:
        extension = ".geojson" if format_name == "geojson" else ".kml"
        label = "GeoJSON" if format_name == "geojson" else "KML"
        output = filedialog.asksaveasfilename(
            parent=self,
            title=f"Export {label}",
            defaultextension=extension,
            filetypes=[(label, f"*{extension}"), ("All files", "*")],
        )
        if not output:
            return
        try:
            if format_name == "geojson":
                text = dumps_json(geojson_document(self._records)) + "\n"
            else:
                text = render_kml(self._records)
            _atomic_write_text(output, text)
        except (OSError, TypeError, ValueError) as exc:
            messagebox.showerror("MetaXtract", f"Location export failed: {exc}", parent=self)
            return
        messagebox.showinfo("MetaXtract", f"Saved: {output}", parent=self)

    def _start_visual_analysis(self) -> None:
        if self._visual_worker is not None or self._files_base is None:
            return
        try:
            threshold = int(self.visual_threshold_var.get())
            if not 0 <= threshold <= 64:
                raise ValueError
        except ValueError:
            messagebox.showwarning(
                "MetaXtract",
                "Visual distance must be an integer from 0 to 64.",
                parent=self,
            )
            return

        for item in self.visual_tree.get_children():
            self.visual_tree.delete(item)
        self.visual_button.configure(state=tk.DISABLED)
        self.visual_threshold.configure(state="disabled")
        self.visual_status_var.set("Hashing local images and comparing appearance…")
        self.visual_progress.start(12)
        self._visual_worker = threading.Thread(
            target=self._run_visual_analysis,
            args=(threshold,),
            name="metaxtract-visual-duplicates",
            daemon=True,
        )
        self._visual_worker.start()
        self._visual_after_id = self.after(75, self._poll_visual_analysis)

    def _run_visual_analysis(self, threshold: int) -> None:
        try:
            result = find_visual_matches(
                self._records,
                self._files_base,
                threshold=threshold,
            )
        except (OSError, TypeError, ValueError) as exc:
            self._visual_events.put(("error", str(exc)))
        except Exception as exc:
            self._visual_events.put(("error", f"{type(exc).__name__}: {exc}"))
        else:
            self._visual_events.put(("done", result))

    def _poll_visual_analysis(self) -> None:
        self._visual_after_id = None
        if self._closed:
            return
        try:
            kind, payload = self._visual_events.get_nowait()
        except queue.Empty:
            self._visual_after_id = self.after(75, self._poll_visual_analysis)
            return

        self._visual_worker = None
        self.visual_progress.stop()
        self.visual_button.configure(state=tk.NORMAL)
        self.visual_threshold.configure(state="normal")
        if kind == "error":
            self.visual_status_var.set("Visual comparison failed.")
            messagebox.showerror("MetaXtract", f"Visual comparison failed: {payload}", parent=self)
            return
        matches, skipped = payload
        self._populate_visual_results(matches, skipped)

    def _populate_visual_results(
        self,
        matches: list[VisualMatch],
        skipped: list[VisualSkip],
    ) -> None:
        for match in matches:
            self.visual_tree.insert(
                "",
                tk.END,
                values=(
                    "Similar",
                    match.first_path,
                    match.second_path,
                    match.hamming_distance,
                    f"{match.first_hash} / {match.second_hash}",
                ),
            )
        for item in skipped:
            self.visual_tree.insert(
                "",
                tk.END,
                values=("Skipped", item.path, item.reason, "", ""),
            )
        self.visual_status_var.set(
            f"Found {len(matches):,} visually similar pair(s); skipped {len(skipped):,} file(s)."
        )

    def _close(self) -> None:
        self._closed = True
        if self._visual_after_id is not None:
            self.after_cancel(self._visual_after_id)
            self._visual_after_id = None
        self.destroy()


def show_analysis(
    parent: tk.Misc,
    records: list[dict[str, Any]],
    files_base: Path | None,
) -> None:
    dialog = AnalysisDialog(parent, records, files_base)
    dialog.grab_set()
    dialog.focus_set()
    parent.wait_window(dialog)
