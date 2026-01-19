from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Optional

import index_store
import utils


@dataclass(frozen=True)
class IndexModel:
    path: str
    ext: str
    has_gps: bool
    has_author: bool
    author: str
    software: str


def _get_bool(d: Any, *keys: str) -> bool:
    cur: Any = d
    for k in keys:
        if not isinstance(cur, dict):
            return False
        cur = cur.get(k)
    return bool(cur)


def _get_str(d: Any, *keys: str) -> str:
    cur: Any = d
    for k in keys:
        if not isinstance(cur, dict):
            return ""
        cur = cur.get(k)
    return str(cur) if isinstance(cur, str) else ""


def load_index(index_path: str) -> tuple[dict[str, Any] | None, list[IndexModel]]:
    session, records = index_store.split_session_and_records(index_path)

    models: list[IndexModel] = []
    for r in records:
        file_obj = r.get("file") if isinstance(r, dict) else None
        if not isinstance(file_obj, dict):
            continue

        path = _get_str(r, "file", "path")
        ext = _get_str(r, "file", "ext")
        if not ext and path:
            # fallback
            dot = path.rfind(".")
            ext = path[dot:] if dot >= 0 else ""

        author = _get_str(r, "identity", "author")
        software = _get_str(r, "capture", "software")
        has_gps = _get_bool(r, "signals", "privacy_flags", "has_gps")
        has_author = _get_bool(r, "signals", "privacy_flags", "has_author") or bool(author.strip())

        models.append(
            IndexModel(
                path=path,
                ext=ext or "",
                has_gps=bool(has_gps),
                has_author=bool(has_author),
                author=author,
                software=software,
            )
        )

    return session, models


def launch(index_path: Optional[str] = None) -> None:
    try:
        import tkinter as tk
        from tkinter import filedialog, ttk
    except Exception as e:
        raise utils.ProcessingError(
            "GUI 실행에 필요한 tkinter를 불러올 수 없습니다(Windows Python에 기본 포함).",
            exit_code=utils.ExitCodes.FAILURE,
            cause=e,
        )

    root = tk.Tk()
    root.title("MetaXtract — Index Viewer")
    root.geometry("1100x650")

    state: dict[str, Any] = {
        "index_path": index_path or "",
        "all": [],
    }

    # Controls
    top = ttk.Frame(root, padding=8)
    top.pack(side=tk.TOP, fill=tk.X)

    path_var = tk.StringVar(value=index_path or "")
    keyword_var = tk.StringVar(value="")
    type_var = tk.StringVar(value="(all)")
    only_gps_var = tk.BooleanVar(value=False)
    only_author_var = tk.BooleanVar(value=False)

    def _open_file() -> None:
        p = filedialog.askopenfilename(
            title="Open index.jsonl",
            filetypes=[("JSONL index", "*.jsonl"), ("All files", "*")],
        )
        if not p:
            return
        path_var.set(p)
        _load_index(p)

    ttk.Button(top, text="Open…", command=_open_file).pack(side=tk.LEFT)
    ttk.Entry(top, textvariable=path_var, width=60).pack(side=tk.LEFT, padx=(8, 12))

    ttk.Label(top, text="Type").pack(side=tk.LEFT)
    type_cb = ttk.Combobox(top, textvariable=type_var, width=12, state="readonly")
    type_cb["values"] = ("(all)",)
    type_cb.pack(side=tk.LEFT, padx=(6, 12))

    ttk.Label(top, text="Keyword").pack(side=tk.LEFT)
    ttk.Entry(top, textvariable=keyword_var, width=25).pack(side=tk.LEFT, padx=(6, 12))

    ttk.Checkbutton(top, text="has_gps", variable=only_gps_var).pack(side=tk.LEFT, padx=(0, 10))
    ttk.Checkbutton(top, text="has_author", variable=only_author_var).pack(side=tk.LEFT, padx=(0, 10))

    # Table
    mid = ttk.Frame(root, padding=(8, 0, 8, 8))
    mid.pack(side=tk.TOP, fill=tk.BOTH, expand=True)

    columns = ("path", "type", "has_gps", "author", "software")
    tree = ttk.Treeview(mid, columns=columns, show="headings")
    tree.heading("path", text="path")
    tree.heading("type", text="type")
    tree.heading("has_gps", text="has_gps")
    tree.heading("author", text="author")
    tree.heading("software", text="software")

    tree.column("path", width=420, anchor=tk.W)
    tree.column("type", width=80, anchor=tk.W)
    tree.column("has_gps", width=70, anchor=tk.CENTER)
    tree.column("author", width=180, anchor=tk.W)
    tree.column("software", width=260, anchor=tk.W)

    vsb = ttk.Scrollbar(mid, orient="vertical", command=tree.yview)
    hsb = ttk.Scrollbar(mid, orient="horizontal", command=tree.xview)
    tree.configure(yscrollcommand=vsb.set, xscrollcommand=hsb.set)

    tree.grid(row=0, column=0, sticky="nsew")
    vsb.grid(row=0, column=1, sticky="ns")
    hsb.grid(row=1, column=0, sticky="ew")

    mid.grid_rowconfigure(0, weight=1)
    mid.grid_columnconfigure(0, weight=1)

    status = ttk.Label(root, padding=8, text="Ready")
    status.pack(side=tk.BOTTOM, fill=tk.X)

    def _set_status(text: str) -> None:
        status.configure(text=text)

    def _apply_filters(*_: Any) -> None:
        items: list[IndexModel] = state.get("all") or []
        ext_filter = type_var.get()
        kw = keyword_var.get().strip().lower()
        only_gps = bool(only_gps_var.get())
        only_author = bool(only_author_var.get())

        filtered: list[IndexModel] = []
        for m in items:
            if ext_filter and ext_filter != "(all)" and (m.ext or "").lower() != ext_filter.lower():
                continue
            if only_gps and not m.has_gps:
                continue
            if only_author and not m.has_author:
                continue
            if kw:
                hay = " ".join([m.path, m.author, m.software]).lower()
                if kw not in hay:
                    continue
            filtered.append(m)

        # refresh
        for iid in tree.get_children():
            tree.delete(iid)

        for m in filtered:
            tree.insert(
                "",
                "end",
                values=(m.path, m.ext, "Y" if m.has_gps else "", m.author, m.software),
            )

        _set_status(f"Shown {len(filtered)} / Total {len(items)}")

    def _load_index(p: str) -> None:
        try:
            _, models = load_index(p)
        except FileNotFoundError as e:
            _set_status(str(e))
            return
        except Exception as e:
            _set_status(f"load failed: {e}")
            return

        state["all"] = models

        exts = sorted({(m.ext or "").lower() for m in models if (m.ext or "").strip()})
        type_cb["values"] = ("(all)", *exts)
        if type_var.get() not in type_cb["values"]:
            type_var.set("(all)")

        _apply_filters()

    # bindings
    type_cb.bind("<<ComboboxSelected>>", _apply_filters)
    keyword_var.trace_add("write", _apply_filters)
    only_gps_var.trace_add("write", _apply_filters)
    only_author_var.trace_add("write", _apply_filters)

    if index_path:
        _load_index(index_path)

    root.mainloop()
