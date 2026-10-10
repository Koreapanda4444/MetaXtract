from __future__ import annotations

import json
import tempfile
import tkinter as tk
import zipfile
from dataclasses import dataclass
from pathlib import Path
from tkinter import filedialog, messagebox, ttk
from typing import Any

from metaxtract.core.jsonio import write_jsonl


@dataclass(frozen=True)
class VerificationRequest:
    bundle: str
    files_base: str | None
    public_key: str | None


@dataclass(frozen=True)
class CaseDetailsResult:
    title: str
    notes: str


def edit_case_details(
    parent: tk.Misc,
    *,
    title: str,
    notes: str,
) -> CaseDetailsResult | None:
    result: CaseDetailsResult | None = None
    dialog = tk.Toplevel(parent)
    dialog.title("Case Notes")
    dialog.transient(parent)
    dialog.resizable(True, True)
    dialog.minsize(460, 320)
    dialog.columnconfigure(0, weight=1)
    dialog.rowconfigure(3, weight=1)

    title_var = tk.StringVar(dialog, value=title)
    ttk.Label(dialog, text="Case title").grid(
        row=0,
        column=0,
        sticky=tk.W,
        padx=12,
        pady=(12, 4),
    )
    title_entry = ttk.Entry(dialog, textvariable=title_var)
    title_entry.grid(row=1, column=0, sticky=tk.EW, padx=12)
    ttk.Label(dialog, text="Notes").grid(
        row=2,
        column=0,
        sticky=tk.W,
        padx=12,
        pady=(12, 4),
    )
    notes_text = tk.Text(dialog, wrap="word", height=10)
    notes_text.grid(row=3, column=0, sticky=tk.NSEW, padx=12)
    notes_text.insert("1.0", notes)

    actions = ttk.Frame(dialog)
    actions.grid(row=4, column=0, sticky=tk.E, padx=12, pady=12)

    def save() -> None:
        nonlocal result
        next_title = title_var.get().strip()
        if not next_title:
            messagebox.showwarning(
                "MetaXtract",
                "Case title cannot be empty.",
                parent=dialog,
            )
            title_entry.focus_set()
            return
        result = CaseDetailsResult(
            title=next_title,
            notes=notes_text.get("1.0", "end-1c"),
        )
        dialog.destroy()

    ttk.Button(actions, text="Cancel", command=dialog.destroy).pack(
        side=tk.RIGHT,
        padx=(6, 0),
    )
    ttk.Button(actions, text="Save", command=save).pack(side=tk.RIGHT)
    dialog.bind("<Escape>", lambda _event: dialog.destroy())
    dialog.protocol("WM_DELETE_WINDOW", dialog.destroy)
    dialog.grab_set()
    title_entry.focus_set()
    parent.wait_window(dialog)
    return result


def export_jsonl(parent: tk.Misc, records: list[dict[str, Any]]) -> None:
    out = filedialog.asksaveasfilename(
        parent=parent,
        defaultextension=".jsonl",
        filetypes=[("JSONL", "*.jsonl")],
    )
    if not out:
        return
    try:
        write_jsonl(out, records)
    except (OSError, TypeError, ValueError) as exc:
        messagebox.showerror("MetaXtract", f"Export failed: {exc}", parent=parent)
        return
    messagebox.showinfo("MetaXtract", f"Saved: {out}", parent=parent)


def export_report(
    parent: tk.Misc,
    records: list[dict[str, Any]],
    format_name: str,
) -> None:
    from metaxtract.reporting.builder import build_report_from_rows
    from metaxtract.reporting.html import render_html

    extension = ".html" if format_name == "html" else ".json"
    label = "HTML" if format_name == "html" else "JSON"
    out = filedialog.asksaveasfilename(
        parent=parent,
        defaultextension=extension,
        filetypes=[(label, f"*{extension}")],
    )
    if not out:
        return
    try:
        if format_name == "html":
            Path(out).write_text(render_html(records) + "\n", encoding="utf-8")
        else:
            write_jsonl(out, [build_report_from_rows(records)])
    except (OSError, TypeError, ValueError) as exc:
        messagebox.showerror("MetaXtract", f"Report export failed: {exc}", parent=parent)
        return
    messagebox.showinfo("MetaXtract", f"Saved: {out}", parent=parent)


def export_case(
    parent: tk.Misc,
    records: list[dict[str, Any]],
    scan_base: Path | None,
) -> None:
    from metaxtract.case.bundle import export_case_bundle

    out = filedialog.asksaveasfilename(
        parent=parent,
        defaultextension=".zip",
        filetypes=[("ZIP archive", "*.zip")],
    )
    if not out:
        return
    include_files = messagebox.askyesnocancel(
        "Case ZIP",
        "Include the original files?\n\n"
        "Yes: include originals\nNo: metadata only\nCancel: stop export",
        parent=parent,
    )
    if include_files is None:
        return
    redact = False
    if not include_files:
        redact = messagebox.askyesno(
            "Case ZIP",
            "Remove privacy-related metadata from the bundle?",
            parent=parent,
        )
    signing_key = None
    if messagebox.askyesno(
        "Case ZIP",
        "Sign the manifest with an Ed25519 key?",
        parent=parent,
    ):
        signing_key = filedialog.askopenfilename(
            parent=parent,
            title="Select private signing key",
            filetypes=[("PEM key", "*.pem"), ("All files", "*")],
        )
        if not signing_key:
            return

    try:
        with tempfile.TemporaryDirectory(prefix="metaxtract-gui-") as temp_dir:
            scan_file = Path(temp_dir) / "scan.jsonl"
            write_jsonl(scan_file, records)
            export_case_bundle(
                scan_file,
                out,
                include_files=include_files,
                redact=redact,
                files_base=scan_base,
                signing_key=signing_key,
            )
    except (OSError, TypeError, ValueError) as exc:
        messagebox.showerror("MetaXtract", f"Case export failed: {exc}", parent=parent)
        return
    messagebox.showinfo("MetaXtract", f"Saved: {out}", parent=parent)


def generate_keypair(parent: tk.Misc) -> None:
    from metaxtract.case.signing import generate_signing_keypair

    private_key = filedialog.asksaveasfilename(
        parent=parent,
        title="Save private signing key",
        defaultextension=".pem",
        filetypes=[("PEM key", "*.pem"), ("All files", "*")],
    )
    if not private_key:
        return
    private_path = Path(private_key)
    public_key = filedialog.asksaveasfilename(
        parent=parent,
        title="Save public verification key",
        initialdir=str(private_path.parent),
        initialfile=f"{private_path.stem}.public.pem",
        defaultextension=".pem",
        filetypes=[("PEM key", "*.pem"), ("All files", "*")],
    )
    if not public_key:
        return
    try:
        generate_signing_keypair(private_key, public_key)
    except (OSError, TypeError, ValueError) as exc:
        messagebox.showerror("MetaXtract", f"Key generation failed: {exc}", parent=parent)
        return
    messagebox.showinfo(
        "MetaXtract",
        f"Private key: {private_key}\nPublic key: {public_key}",
        parent=parent,
    )


def choose_verification(parent: tk.Misc) -> VerificationRequest | None:
    bundle = filedialog.askopenfilename(
        parent=parent,
        title="Select case ZIP",
        filetypes=[("ZIP archive", "*.zip"), ("All files", "*")],
    )
    if not bundle:
        return None
    try:
        with zipfile.ZipFile(bundle, "r") as archive:
            signed = "signature.json" in archive.namelist()
    except (OSError, zipfile.BadZipFile) as exc:
        messagebox.showerror("MetaXtract", f"Invalid ZIP: {exc}", parent=parent)
        return None

    public_key = None
    if signed:
        public_key = filedialog.askopenfilename(
            parent=parent,
            title="Select public verification key",
            filetypes=[("PEM key", "*.pem"), ("All files", "*")],
        )
        if not public_key:
            return None

    files_base = None
    if messagebox.askyesno(
        "Verify ZIP",
        "Also compare records with original files in a folder?",
        parent=parent,
    ):
        files_base = filedialog.askdirectory(
            parent=parent,
            title="Select original files folder",
        )
        if not files_base:
            return None
    return VerificationRequest(bundle, files_base, public_key)


def show_verification_issues(
    parent: tk.Misc,
    bundle: str,
    issues: list[dict[str, str]],
) -> None:
    window = tk.Toplevel(parent)
    window.title("Bundle verification issues")
    window.geometry("760x480")
    window.minsize(520, 320)
    ttk.Label(
        window,
        text=f"{Path(bundle).name}: {len(issues)} issue(s)",
        padding=10,
    ).pack(anchor=tk.W)
    text_frame = ttk.Frame(window, padding=(10, 0, 10, 10))
    text_frame.pack(fill=tk.BOTH, expand=True)
    output = tk.Text(text_frame, wrap="none")
    scrollbar = ttk.Scrollbar(text_frame, orient=tk.VERTICAL, command=output.yview)
    output.configure(yscrollcommand=scrollbar.set)
    output.pack(side=tk.LEFT, fill=tk.BOTH, expand=True)
    scrollbar.pack(side=tk.RIGHT, fill=tk.Y)
    output.insert(
        "1.0",
        json.dumps(issues, ensure_ascii=False, sort_keys=True, indent=2) + "\n",
    )
    output.configure(state=tk.DISABLED)
