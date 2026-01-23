from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Any, Optional

import queue
import threading

import config
import engine
import index_store
import report
import utils


@dataclass(frozen=True)
class IndexModel:
    path: str
    ext: str
    has_gps: bool
    has_author: bool
    author: str
    software: str
    record: dict[str, Any]


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
        _ = author  # F841 미사용 변수 소비
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
                record=r if isinstance(r, dict) else {},
            )
        )

    return session, models


def launch(index_path: Optional[str] = None) -> None:
    try:
        import tkinter as tk
        from tkinter import filedialog, messagebox, simpledialog, ttk
    except Exception as e:
        raise utils.ProcessingError(
            "GUI 실행에 필요한 tkinter를 불러올 수 없습니다(Windows Python에 기본 포함).",
            exit_code=utils.ExitCodes.FAILURE,
            cause=e,
        )

    root = tk.Tk()
    root.title("MetaXtract — Index Viewer")
    root.geometry("1200x700")

    state: dict[str, Any] = {
        "index_path": index_path or "",
        "session": None,
        "all": [],
        "filtered": [],
        "iid_to_model": {},
        "sort": {"col": "path", "desc": False},
        "thumb": None,
        "scan_thread": None,
        "scan_cancel": None,
        "scan_queue": None,
    }

    # Controls
    top = ttk.Frame(root, padding=8)
    top.pack(side=tk.TOP, fill=tk.X)

    path_var = tk.StringVar(value=index_path or "")
    keyword_var = tk.StringVar(value="")
    type_var = tk.StringVar(value="(all)")
    only_gps_var = tk.BooleanVar(value=False)
    only_author_var = tk.BooleanVar(value=False)
    template_var = tk.StringVar(value="privacy")
    redact_var = tk.BooleanVar(value=False)

    # column visibility
    show_col: dict[str, tk.BooleanVar] = {
        "path": tk.BooleanVar(value=True),
        "type": tk.BooleanVar(value=True),
        "has_gps": tk.BooleanVar(value=True),
        "author": tk.BooleanVar(value=True),
        "software": tk.BooleanVar(value=True),
    }

    def _open_file() -> None:
        p = filedialog.askopenfilename(
            title="Open index.jsonl",
            filetypes=[("JSONL index", "*.jsonl"), ("All files", "*")],
        )
        if not p:
            return
        path_var.set(p)
        _load_index(p)

    def _start_scan() -> None:
        if state.get("scan_thread") is not None:
            _set_status("이미 스캔이 실행 중입니다.")
            return

        root_dir = filedialog.askdirectory(title="Select folder to scan")
        if not root_dir:
            return

        out_path = filedialog.asksaveasfilename(
            title="Save index.jsonl",
            defaultextension=".jsonl",
            filetypes=[("JSONL index", "*.jsonl"), ("All files", "*")],
        )
        if not out_path:
            return

        recursive = bool(messagebox.askyesno("Scan", "재귀적으로(하위 폴더 포함) 스캔할까요?"))
        default_threads = config.Settings().threads
        threads = simpledialog.askinteger(
            "Scan",
            "스레드 수 (--threads)",
            initialvalue=int(default_threads),
            minvalue=1,
            maxvalue=64,
        )
        if threads is None:
            return

        cancel = utils.CancelToken()
        q: queue.Queue[tuple[str, Any]] = queue.Queue()
        state["scan_cancel"] = cancel
        state["scan_queue"] = q

        scan_btn.configure(state="disabled")
        stop_btn.configure(state="normal")
        _set_status(f"Scanning… root={root_dir} threads={threads}")

        def _worker() -> None:
            processed = 0
            file_error_files = 0
            try:
                exclude: list[str] = []
                header = engine.make_session_header(
                    root=str(root_dir),
                    recursive=recursive,
                    include=None,
                    exclude=exclude,
                    hash_algo="none",
                    threads=int(threads),
                )

                with index_store.JsonlWriter(out_path, append=False) as w:
                    w.write(header)

                    for rec in engine.scan_iter(
                        str(root_dir),
                        recursive=recursive,
                        include_exts=None,
                        exclude_patterns=exclude,
                        hash_algo="none",
                        redact=False,
                        threads=int(threads),
                        cancel=cancel,
                    ):
                        processed += 1
                        raw = rec.get("raw") if isinstance(rec, dict) else None
                        errs = raw.get("errors") if isinstance(raw, dict) else None
                        if isinstance(errs, list) and errs:
                            file_error_files += 1

                        w.write(rec)

                        if processed % 20 == 0:
                            q.put(("progress", (processed, file_error_files)))

                q.put(("done", (processed, file_error_files, bool(cancel.is_cancelled()))))
            except Exception as e:
                q.put(("error", str(e)))

        th = threading.Thread(target=_worker, daemon=True)
        state["scan_thread"] = th
        th.start()

        def _poll() -> None:
            q2 = state.get("scan_queue")
            if q2 is None:
                return
            try:
                while True:
                    kind, payload = q2.get_nowait()
                    if kind == "progress":
                        p, fe = payload
                        _set_status(f"Scanning… processed={p} failed_files={fe}")
                    elif kind == "done":
                        p, fe, was_cancelled = payload
                        _set_status(
                            f"Scan {'cancelled' if was_cancelled else 'done'}: processed={p} failed_files={fe} → {out_path}"
                        )
                        # 완료 시 생성된 인덱스를 자동 로드
                        path_var.set(out_path)
                        _load_index(out_path)
                        state["scan_thread"] = None
                        state["scan_cancel"] = None
                        state["scan_queue"] = None
                        scan_btn.configure(state="normal")
                        stop_btn.configure(state="disabled")
                    elif kind == "error":
                        _set_status(f"scan failed: {payload}")
                        state["scan_thread"] = None
                        state["scan_cancel"] = None
                        state["scan_queue"] = None
                        scan_btn.configure(state="normal")
                        stop_btn.configure(state="disabled")
            except queue.Empty:
                pass

            if state.get("scan_thread") is not None:
                root.after(150, _poll)

        root.after(150, _poll)

    def _stop_scan() -> None:
        cancel = state.get("scan_cancel")
        if cancel is None:
            return
        cancel.cancel()
        _set_status("Cancel requested…")
        stop_btn.configure(state="disabled")

    ttk.Button(top, text="Open…", command=_open_file).pack(side=tk.LEFT)
    ttk.Entry(top, textvariable=path_var, width=60).pack(side=tk.LEFT, padx=(8, 12))

    scan_btn = ttk.Button(top, text="Scan…", command=_start_scan)
    scan_btn.pack(side=tk.LEFT, padx=(0, 6))
    stop_btn = ttk.Button(top, text="Stop", command=_stop_scan, state="disabled")
    stop_btn.pack(side=tk.LEFT, padx=(0, 12))

    ttk.Label(top, text="Type").pack(side=tk.LEFT)
    type_cb = ttk.Combobox(top, textvariable=type_var, width=12, state="readonly")
    type_cb["values"] = ("(all)",)
    type_cb.pack(side=tk.LEFT, padx=(6, 12))

    ttk.Label(top, text="Keyword").pack(side=tk.LEFT)
    ttk.Entry(top, textvariable=keyword_var, width=25).pack(side=tk.LEFT, padx=(6, 12))

    ttk.Checkbutton(top, text="has_gps", variable=only_gps_var).pack(side=tk.LEFT, padx=(0, 10))
    ttk.Checkbutton(top, text="has_author", variable=only_author_var).pack(side=tk.LEFT, padx=(0, 10))

    ttk.Label(top, text="Template").pack(side=tk.LEFT)
    tpl_cb = ttk.Combobox(top, textvariable=template_var, width=10, state="readonly")
    tpl_cb["values"] = ("privacy", "forensics", "content")
    tpl_cb.pack(side=tk.LEFT, padx=(6, 8))
    ttk.Checkbutton(top, text="redact", variable=redact_var).pack(side=tk.LEFT, padx=(0, 10))

    def _open_columns_dialog() -> None:
        win = tk.Toplevel(root)
        win.title("Columns")
        win.transient(root)
        win.resizable(False, False)

        body = ttk.Frame(win, padding=10)
        body.pack(fill=tk.BOTH, expand=True)

        ttk.Label(body, text="표시할 컬럼을 선택하세요.").pack(anchor=tk.W)
        for k in ("path", "type", "has_gps", "author", "software"):
            ttk.Checkbutton(body, text=k, variable=show_col[k], command=lambda: _apply_column_visibility()).pack(
                anchor=tk.W
            )

        ttk.Button(body, text="Close", command=win.destroy).pack(anchor=tk.E, pady=(8, 0))

    ttk.Button(top, text="Columns…", command=_open_columns_dialog).pack(side=tk.LEFT, padx=(0, 10))

    # Main split (table + preview)
    main = ttk.Panedwindow(root, orient="horizontal")
    main.pack(side=tk.TOP, fill=tk.BOTH, expand=True, padx=8, pady=(0, 8))

    left = ttk.Frame(main)
    right = ttk.Frame(main)
    main.add(left, weight=3)
    main.add(right, weight=2)

    # Table
    columns = ("path", "type", "has_gps", "author", "software")
    tree = ttk.Treeview(left, columns=columns, show="headings")
    def _on_sort(col: str) -> None:
        cur = state.get("sort") or {}
        if cur.get("col") == col:
            cur["desc"] = not bool(cur.get("desc"))
        else:
            cur["col"] = col
            cur["desc"] = False
        state["sort"] = cur
        _apply_filters()

    tree.heading("path", text="path", command=lambda: _on_sort("path"))
    tree.heading("type", text="type", command=lambda: _on_sort("type"))
    tree.heading("has_gps", text="has_gps", command=lambda: _on_sort("has_gps"))
    tree.heading("author", text="author", command=lambda: _on_sort("author"))
    tree.heading("software", text="software", command=lambda: _on_sort("software"))

    tree.column("path", width=420, anchor=tk.W)
    tree.column("type", width=80, anchor=tk.W)
    tree.column("has_gps", width=70, anchor=tk.CENTER)
    tree.column("author", width=180, anchor=tk.W)
    tree.column("software", width=260, anchor=tk.W)

    vsb = ttk.Scrollbar(left, orient="vertical", command=tree.yview)
    hsb = ttk.Scrollbar(left, orient="horizontal", command=tree.xview)
    tree.configure(yscrollcommand=vsb.set, xscrollcommand=hsb.set)

    tree.grid(row=0, column=0, sticky="nsew")
    vsb.grid(row=0, column=1, sticky="ns")
    hsb.grid(row=1, column=0, sticky="ew")

    left.grid_rowconfigure(0, weight=1)
    left.grid_columnconfigure(0, weight=1)

    # Preview panel
    pv = ttk.Frame(right, padding=8)
    pv.pack(fill=tk.BOTH, expand=True)

    pv_title = ttk.Label(pv, text="Preview", font=("Segoe UI", 11, "bold"))
    pv_title.pack(anchor=tk.W)

    thumb_label = ttk.Label(pv)
    thumb_label.pack(anchor=tk.W, pady=(6, 8))

    txt = tk.Text(pv, wrap="word", height=20)
    txt.pack(fill=tk.BOTH, expand=True)
    txt.configure(state="disabled")

    export_row = ttk.Frame(pv)
    export_row.pack(fill=tk.X, pady=(8, 0))

    def _export(fmt: str) -> None:
        selected = tree.selection()
        if not selected:
            _set_status("선택된 항목이 없습니다(내보내기 실패).")
            return

        models: list[IndexModel] = []
        iid_map: dict[str, IndexModel] = state.get("iid_to_model") or {}
        for iid in selected:
            m = iid_map.get(iid)
            if m is not None:
                models.append(m)

        if not models:
            _set_status("선택된 항목을 찾지 못했습니다.")
            return

        ext = ".txt" if fmt == "txt" else ".csv"
        out_path = filedialog.asksaveasfilename(
            title=f"Export {fmt.upper()}",
            defaultextension=ext,
            filetypes=[(f"{fmt.upper()} file", f"*{ext}"), ("All files", "*")],
        )
        if not out_path:
            return

        session = state.get("session")
        records = [m.record for m in models]
        try:
            out = report.generate_from_records(
                session if isinstance(session, dict) else None,
                records,
                fmt=fmt,
                template=template_var.get(),
                redact=bool(redact_var.get()),
            )
            Path(out_path).write_text(out, encoding="utf-8")
        except Exception as e:
            _set_status(f"export failed: {e}")
            return

        _set_status(f"Exported {len(records)} items → {out_path}")

    ttk.Button(export_row, text="Export TXT", command=lambda: _export("txt")).pack(side=tk.LEFT)
    ttk.Button(export_row, text="Export CSV", command=lambda: _export("csv")).pack(side=tk.LEFT, padx=(8, 0))

    status = ttk.Label(root, padding=8, text="Ready")
    status.pack(side=tk.BOTTOM, fill=tk.X)

    def _set_status(text: str) -> None:
        status.configure(text=text)

    def _apply_column_visibility() -> None:
        display = [c for c in columns if bool(show_col[c].get())]
        if not display:
            # 최소 1개는 보여주기
            show_col["path"].set(True)
            display = ["path"]
        tree["displaycolumns"] = tuple(display)

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

        # sort
        s = state.get("sort") or {}
        col = str(s.get("col") or "path")
        desc = bool(s.get("desc"))

        def _sort_key(m: IndexModel) -> Any:
            if col == "type":
                return (m.ext or "").lower()
            if col == "has_gps":
                return 1 if m.has_gps else 0
            if col == "author":
                return (m.author or "").lower()
            if col == "software":
                return (m.software or "").lower()
            return (m.path or "").lower()

        filtered.sort(key=_sort_key, reverse=desc)
        state["filtered"] = filtered

        # refresh
        for iid in tree.get_children():
            tree.delete(iid)

        iid_to_model: dict[str, IndexModel] = {}

        for i, m in enumerate(filtered):
            iid = str(i)
            tree.insert(
                "",
                "end",
                iid=iid,
                values=(m.path, m.ext, "Y" if m.has_gps else "", m.author, m.software),
            )
            iid_to_model[iid] = m

        state["iid_to_model"] = iid_to_model

        _apply_column_visibility()

        _set_status(f"Shown {len(filtered)} / Total {len(items)}")

    def _resolve_file_path(session: Any, rec_path: str) -> Optional[Path]:
        if not rec_path:
            return None
        # absolute Windows path or absolute unix-ish path
        if ":\\" in rec_path or rec_path.startswith("/"):
            return Path(rec_path)

        base = None
        try:
            scan = session.get("scan") if isinstance(session, dict) else None
            root = scan.get("root") if isinstance(scan, dict) else None
            if isinstance(root, str) and root:
                rp = Path(root)
                if rp.is_file():
                    base = rp.parent
                else:
                    base = rp
        except Exception:
            base = None

        if base is None:
            return None
        return (base / rec_path)

    def _set_preview_text(text_value: str) -> None:
        txt.configure(state="normal")
        txt.delete("1.0", tk.END)
        txt.insert("1.0", text_value)
        txt.configure(state="disabled")

    def _update_preview(m: Optional[IndexModel]) -> None:
        state["thumb"] = None
        thumb_label.configure(image="")

        if m is None:
            pv_title.configure(text="Preview")
            _set_preview_text("")
            return

        pv_title.configure(text=f"Preview — {m.path}")

        r = m.record
        identity = r.get("identity") if isinstance(r.get("identity"), dict) else {}
        capture = r.get("capture") if isinstance(r.get("capture"), dict) else {}
        geo = r.get("geo") if isinstance(r.get("geo"), dict) else {}
        meta_times = r.get("meta_times") if isinstance(r.get("meta_times"), dict) else {}
        sig = r.get("signals") if isinstance(r.get("signals"), dict) else {}
        flags = sig.get("privacy_flags") if isinstance(sig.get("privacy_flags"), dict) else {}

        lines: list[str] = []
        lines.append(f"path: {m.path}")
        lines.append(f"type: {m.ext}")
        if m.author:
            lines.append(f"author: {m.author}")
        if m.software:
            lines.append(f"software: {m.software}")
        if isinstance(capture.get("make"), str) and capture.get("make"):
            lines.append(f"make: {capture.get('make')}")
        if isinstance(capture.get("model"), str) and capture.get("model"):
            lines.append(f"model: {capture.get('model')}")
        if isinstance(capture.get("datetime_original"), str) and capture.get("datetime_original"):
            lines.append(f"datetime_original: {capture.get('datetime_original')}")
        if isinstance(meta_times.get("created"), str) and meta_times.get("created"):
            lines.append(f"meta.created: {meta_times.get('created')}")
        if isinstance(meta_times.get("modified"), str) and meta_times.get("modified"):
            lines.append(f"meta.modified: {meta_times.get('modified')}")
        if isinstance(meta_times.get("digitized"), str) and meta_times.get("digitized"):
            lines.append(f"meta.digitized: {meta_times.get('digitized')}")

        if geo:
            lat = geo.get("lat")
            lon = geo.get("lon")
            if isinstance(lat, (int, float)) and isinstance(lon, (int, float)):
                lines.append(f"gps: {lat}, {lon}")

        if flags:
            # 핵심 플래그만 요약
            lines.append(
                "flags: "
                + " ".join(
                    [
                        f"has_gps={bool(flags.get('has_gps'))}",
                        f"has_author={bool(flags.get('has_author'))}",
                        f"has_software_trace={bool(flags.get('has_software_trace'))}",
                    ]
                )
            )

        # structured errors
        raw = r.get("raw") if isinstance(r.get("raw"), dict) else {}
        errs = raw.get("errors") if isinstance(raw.get("errors"), list) else []
        if errs:
            lines.append("errors:")
            for it in errs[:10]:
                if not isinstance(it, dict):
                    continue
                stage = str(it.get("stage") or "")
                code = str(it.get("error_code") or "")
                msg = str(it.get("message_short") or "")
                lines.append(f"- [{stage}] {code}: {msg}".strip())
            if len(errs) > 10:
                lines.append(f"(+{len(errs) - 10} more)")

        _set_preview_text("\n".join(lines) + "\n")

        # Thumbnail for images
        if (m.ext or "").lower() in {".jpg", ".jpeg", ".png"}:
            try:
                session_obj = state.get("session")
                full = _resolve_file_path(session_obj, m.path)
                if full is None or not full.exists():
                    return
                from PIL import Image, ImageTk

                with Image.open(full) as im:
                    im.thumbnail((360, 360))
                    photo = ImageTk.PhotoImage(im)
                state["thumb"] = photo
                thumb_label.configure(image=photo)
            except Exception:
                # 미리보기 실패는 치명적이지 않음
                return

    def _load_index(p: str) -> None:
        try:
            session, models = load_index(p)
        except FileNotFoundError as e:
            _set_status(str(e))
            return
        except Exception as e:
            _set_status(f"load failed: {e}")
            return

        state["session"] = session
        state["all"] = models

        exts = sorted({(m.ext or "").lower() for m in models if (m.ext or "").strip()})
        type_cb["values"] = ("(all)", *exts)
        if type_var.get() not in type_cb["values"]:
            type_var.set("(all)")

        _apply_filters()

        # reset preview
        _update_preview(None)

    # bindings
    type_cb.bind("<<ComboboxSelected>>", _apply_filters)
    keyword_var.trace_add("write", _apply_filters)
    only_gps_var.trace_add("write", _apply_filters)
    only_author_var.trace_add("write", _apply_filters)

    def _on_select(_: Any) -> None:
        sel = tree.selection()
        if not sel:
            _update_preview(None)
            return
        iid = sel[0]
        m = (state.get("iid_to_model") or {}).get(iid)
        _update_preview(m)

    tree.bind("<<TreeviewSelect>>", _on_select)

    if index_path:
        _load_index(index_path)

    _apply_column_visibility()

    root.mainloop()
