from metaxtract.report_html import render_html


def sample_records():
    return [
        {
            "path": "a.jpg",
            "mime": "image/jpeg",
            "size_bytes": 1234,
            "metadata": {"author": "alice", "gps": [37.5, 127.0]},
            "warnings": ["low_resolution"],
            "errors": [],
        },
        {
            "path": "b.docx",
            "mime": "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "size_bytes": 4321,
            "metadata": {"author": "bob", "producer": "Word"},
            "warnings": [],
            "errors": ["corrupt"],
        },
    ]


def test_render_html_basic():
    html = render_html(sample_records())
    assert "MetaXtract Report" in html
    assert "파일 개수" in html
    assert "image/jpeg" in html
    assert "application/vnd.openxmlformats-officedocument.wordprocessingml.document" in html
    assert "alice" in html
    assert "bob" in html
    assert "Warnings" in html
    assert "Errors" in html
    assert "low_resolution" in html
    assert "corrupt" in html


def test_render_html_uses_extractor_field_names_and_escapes_rows():
    records = [
        {
            "path": "photo.jpg",
            "mime": "image/jpeg",
            "size_bytes": "<size>",
            "metadata": {
                "gps_latitude": 37.5,
                "gps_longitude": 127.0,
                "exif_model": "Camera X",
                "pdf_author": "PDF Author",
                "docx_author": "DOCX Author",
                "pdf_producer": "PDF Tool",
            },
            "warnings": ["<script>alert(1)</script>"],
            "errors": ["<img src=x onerror=alert(1)>"],
        }
    ]

    html = render_html(records)

    assert "GPS 정보 포함 파일" in html
    assert "PDF Author" in html
    assert "DOCX Author" in html
    assert "PDF Tool" in html
    assert "Camera X" in html
    assert "<script>" not in html
    assert "<img src=x" not in html
    assert "&lt;size&gt;" in html
